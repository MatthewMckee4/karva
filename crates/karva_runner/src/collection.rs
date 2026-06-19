use karva_collector::{
    CollectedModule, CollectedPackage, CollectionSettings, ModuleType, collect_file,
};

use std::thread;

use anyhow::{Context as _, Result};
use camino::{Utf8Path, Utf8PathBuf};
use crossbeam_channel::unbounded;
use ignore::types::Types;
use ignore::{WalkBuilder, WalkState};
use karva_project::path::{TestPath, TestPathFunction};

/// Collector used for collecting all test functions and fixtures in a package.
///
/// This is only used in the main `karva` cli.
/// If we used this in the `karva-worker` cli, this would be very inefficient.
pub struct ParallelCollector<'a> {
    cwd: &'a Utf8Path,
    settings: CollectionSettings<'a>,
}

enum CollectionMessage {
    Module(CollectedModule),
    Error(anyhow::Error),
}

impl<'a> ParallelCollector<'a> {
    pub fn new(cwd: &'a Utf8Path, settings: CollectionSettings<'a>) -> Self {
        Self { cwd, settings }
    }

    /// Collect from a directory in parallel using `WalkParallel`.
    pub(crate) fn collect_directory(&self, path: &Utf8PathBuf) -> Result<CollectedPackage> {
        let (tx, rx) = unbounded::<CollectionMessage>();

        let cloned_path = path.clone();

        // Spawn receiver thread to collect results
        let receiver_handle = thread::spawn(move || {
            let mut package = CollectedPackage::new(cloned_path);
            let mut first_error = None;

            for message in rx {
                match message {
                    CollectionMessage::Module(collected_module) => {
                        match collected_module.module_type() {
                            ModuleType::Test => {
                                package.add_module(collected_module);
                            }
                            ModuleType::Configuration => {
                                package.add_configuration_module(collected_module);
                            }
                        }
                    }
                    CollectionMessage::Error(error) => {
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
            }

            if let Some(error) = first_error {
                Err(error)
            } else {
                Ok(package)
            }
        });

        let walker = self.create_parallel_walker(path)?;

        walker.run(|| {
            let tx = tx.clone();

            Box::new(move |entry| {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        if let Err(err) = tx.send(CollectionMessage::Error(
                            anyhow::Error::new(error).context("failed to walk test directory"),
                        )) {
                            tracing::warn!("failed to send walk error from worker thread: {err}");
                        }
                        return WalkState::Quit;
                    }
                };

                if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                    return WalkState::Continue;
                }

                let Ok(file_path) = Utf8PathBuf::from_path_buf(entry.path().to_path_buf()) else {
                    return WalkState::Continue;
                };

                match collect_file(&file_path, self.cwd, &self.settings, &[]) {
                    Ok(Some(module)) => {
                        if let Err(err) = tx.send(CollectionMessage::Module(module)) {
                            tracing::warn!(
                                "failed to send collected module from worker thread: {err}"
                            );
                            return WalkState::Quit;
                        }
                    }
                    Ok(None) => {}
                    Err(error) => {
                        if let Err(err) = tx.send(CollectionMessage::Error(error.into())) {
                            tracing::warn!(
                                "failed to send collection error from worker thread: {err}"
                            );
                            return WalkState::Quit;
                        }
                    }
                }

                WalkState::Continue
            })
        });

        // Drop the original sender to close the channel
        drop(tx);

        let package = receiver_handle
            .join()
            .map_err(|_| anyhow::anyhow!("Test collection thread panicked"))??;

        Ok(package)
    }

    /// Collect from all paths and build a complete package structure.
    pub fn collect_all(&self, test_paths: Vec<TestPath>) -> Result<CollectedPackage> {
        let mut session_package = CollectedPackage::new(self.cwd.to_path_buf());

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
                        collect_file(&file_path, self.cwd, &self.settings, &[function_name])?
                    {
                        session_package.add_module(module);
                    }
                }
                TestPath::File(file_path) => {
                    if let Some(module) = collect_file(&file_path, self.cwd, &self.settings, &[])? {
                        session_package.add_module(module);
                    }
                }
            }
        }

        session_package.shrink();

        Ok(session_package)
    }

    /// Creates a configured parallel directory walker for Python file discovery.
    fn create_parallel_walker(&self, path: &Utf8PathBuf) -> Result<ignore::WalkParallel> {
        let num_threads = karva_static::max_parallelism()
            .context("Failed to determine collection worker count")?
            .get();

        Ok(WalkBuilder::new(path)
            .threads(num_threads)
            .standard_filters(true)
            .require_git(false)
            .git_global(false)
            .parents(true)
            .follow_links(true)
            .git_ignore(self.settings.respect_ignore_files)
            .types(python_file_types()?)
            .build_parallel())
    }
}

fn python_file_types() -> Result<Types> {
    let mut types = ignore::types::TypesBuilder::new();
    types
        .add("python", "*.py")
        .context("failed to register Python file pattern")?;
    types.select("python");
    types
        .build()
        .context("failed to build Python file type matcher")
}

#[cfg(test)]
mod tests {
    use super::*;

    use ruff_python_ast::PythonVersion;

    fn temp_path(dir: &tempfile::TempDir) -> &Utf8Path {
        Utf8Path::from_path(dir.path()).expect("temp path should be UTF-8")
    }

    fn collection_settings() -> CollectionSettings<'static> {
        CollectionSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test_",
            respect_ignore_files: true,
            collect_fixtures: false,
        }
    }

    #[cfg(unix)]
    #[test]
    fn collect_directory_reports_walker_errors() {
        use std::os::unix::fs::symlink;

        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let root = temp_path(&temp_dir);
        let test_path = root.join("test_sample.py");
        std::fs::write(&test_path, "def test_sample(): pass\n").expect("write test file");
        symlink(root, root.join("loop")).expect("create symlink loop");

        let collector = ParallelCollector::new(root, collection_settings());

        let error = collector
            .collect_directory(&root.to_path_buf())
            .expect_err("walker loop should fail collection");

        let error = error.to_string();
        assert!(
            error.contains("failed to walk test directory"),
            "unexpected error: {error}"
        );
    }
}
