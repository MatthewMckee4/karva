use karva_collector::{
    CollectedModule, CollectedPackage, CollectionSettings, ModuleType, collect_file,
};

use std::thread;

use anyhow::Result;
use camino::Utf8PathBuf;
use crossbeam_channel::unbounded;
use ignore::{WalkBuilder, WalkState};
use karva_system::{
    System,
    path::{TestPath, TestPathFunction},
};

/// Collector used for collecting all test functions and fixtures in a package.
///
/// This is only used in the main `karva` cli.
/// If we used this in the `karva-worker` cli, this would be very inefficient.
pub struct ParallelCollector<'a> {
    system: &'a dyn System,
    settings: CollectionSettings<'a>,
}

impl<'a> ParallelCollector<'a> {
    pub fn new(system: &'a dyn System, settings: CollectionSettings<'a>) -> Self {
        Self { system, settings }
    }

    /// Collect from a directory in parallel using `WalkParallel`.
    pub(crate) fn collect_directory(&self, path: &Utf8PathBuf) -> Result<CollectedPackage> {
        let (tx, rx) = unbounded::<CollectedModule>();

        let cloned_path = path.clone();

        // Spawn receiver thread to collect results
        let receiver_handle = thread::spawn(move || {
            let mut package = CollectedPackage::new(cloned_path);

            for collected_module in rx {
                match collected_module.module_type() {
                    ModuleType::Test => {
                        package.add_module(collected_module);
                    }
                    ModuleType::Configuration => {
                        package.add_configuration_module(collected_module);
                    }
                }
            }

            package
        });

        let walker = self.create_parallel_walker(&path.clone());

        walker.run(|| {
            let tx = tx.clone();

            Box::new(move |entry| {
                let Ok(entry) = entry else {
                    return WalkState::Continue;
                };

                if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                    return WalkState::Continue;
                }

                let Ok(file_path) = Utf8PathBuf::from_path_buf(entry.path().to_path_buf()) else {
                    return WalkState::Continue;
                };

                if let Some(module) = collect_file(&file_path, self.system, &self.settings, &[]) {
                    let _ = tx.send(module);
                }

                WalkState::Continue
            })
        });

        // Drop the original sender to close the channel
        drop(tx);

        receiver_handle
            .join()
            .map_err(|_| anyhow::anyhow!("Test collection thread panicked"))
    }

    /// Collect from all paths and build a complete package structure.
    pub fn collect_all(&self, test_paths: Vec<TestPath>) -> Result<CollectedPackage> {
        let mut session_package =
            CollectedPackage::new(self.system.current_directory().to_path_buf());

        for path in test_paths {
            match path {
                TestPath::Directory(dir_path) => {
                    let package = self.collect_directory(&dir_path)?;
                    session_package.add_package(package);
                }
                TestPath::Function(TestPathFunction {
                    path: file_path,
                    function_name,
                }) => {
                    if let Some(module) =
                        collect_file(&file_path, self.system, &self.settings, &[function_name])
                    {
                        session_package.add_module(module);
                    }
                }
                TestPath::File(file_path) => {
                    if let Some(module) = collect_file(&file_path, self.system, &self.settings, &[])
                    {
                        session_package.add_module(module);
                    }
                }
            }
        }

        session_package.shrink();

        Ok(session_package)
    }

    /// Creates a configured parallel directory walker for Python file discovery.
    fn create_parallel_walker(&self, path: &Utf8PathBuf) -> ignore::WalkParallel {
        let num_threads = karva_system::max_parallelism().get();

        WalkBuilder::new(path)
            .threads(num_threads)
            .standard_filters(true)
            .require_git(false)
            .git_global(false)
            .parents(true)
            .follow_links(true)
            .git_ignore(self.settings.respect_ignore_files)
            .types({
                let mut types = ignore::types::TypesBuilder::new();
                types.add("python", "*.py").unwrap();
                types.select("python");
                types.build().unwrap()
            })
            .build_parallel()
    }
}
