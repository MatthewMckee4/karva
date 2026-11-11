use karva_collector::{
    CollectedModule, CollectedPackage, CollectionSettings, ModuleType, collect_file_for_scheduling,
};

use std::collections::{BTreeMap, BTreeSet};
use std::thread;

use karva_collector::collect_file;

use anyhow::{Context as _, Result};
use camino::{Utf8Path, Utf8PathBuf};
use crossbeam_channel::unbounded;
use ignore::types::Types;
use ignore::{WalkBuilder, WalkState};
use karva_project::path::{TestPath, TestPathFunction};

/// Discovers test modules concurrently in the controller process.
///
/// Workers receive already-selected test paths, so repeating this directory
/// walk inside each worker would multiply collection cost.
pub struct ParallelCollector<'a> {
    /// Project root used to resolve collected module names.
    cwd: &'a Utf8Path,

    /// Syntax and discovery rules shared with single-file collection.
    settings: CollectionSettings<'a>,
}

enum CollectionMessage {
    Module(CollectedModule),
    Error(anyhow::Error),
}

impl<'a> ParallelCollector<'a> {
    /// Creates a controller-side collector rooted at `cwd`.
    pub fn new(cwd: &'a Utf8Path, settings: CollectionSettings<'a>) -> Self {
        Self { cwd, settings }
    }

    /// Collects complete source for inspection using the runner's directory filters.
    ///
    /// Unlike scheduling, inspection retains modules with no runnable tests so
    /// class and conditional definitions can still be diagnosed. No Python runs.
    pub fn collect_sources(&self, paths: Vec<TestPath>) -> Result<Vec<CollectedModule>> {
        let mut files: BTreeMap<Utf8PathBuf, Option<Vec<String>>> = BTreeMap::new();
        for path in paths {
            match path {
                TestPath::Directory(path) => {
                    let (tx, rx) = unbounded();
                    self.create_parallel_walker(&path)?.run(|| {
                        let tx = tx.clone();
                        Box::new(move |entry| {
                            let result = entry.map_err(anyhow::Error::from).and_then(|entry| {
                                if entry.file_type().is_some_and(|kind| kind.is_file()) {
                                    Utf8PathBuf::from_path_buf(entry.into_path())
                                        .map(Some)
                                        .map_err(|path| {
                                            anyhow::anyhow!(
                                                "non-Unicode source path: {}",
                                                path.display()
                                            )
                                        })
                                } else {
                                    Ok(None)
                                }
                            });
                            if tx.send(result).is_err() {
                                return WalkState::Quit;
                            }
                            WalkState::Continue
                        })
                    });
                    drop(tx);
                    for result in rx {
                        if let Some(path) = result? {
                            files.insert(path, None);
                        }
                    }
                }
                TestPath::File(path) => {
                    files.insert(path, None);
                }
                TestPath::Function(function) => {
                    files
                        .entry(function.path)
                        .and_modify(|names| {
                            if let Some(names) = names {
                                names.push(function.function_name.clone());
                            }
                        })
                        .or_insert_with(|| Some(vec![function.function_name]));
                }
            }
        }
        // Like worker discovery, selected modules inherit ancestor configuration
        // even when an ignore pattern excludes the conftest file itself.
        let mut visited = BTreeSet::new();
        let selected = files.keys().cloned().collect::<Vec<_>>();
        for path in selected {
            for parent in path
                .ancestors()
                .skip(1)
                .take_while(|path| path.starts_with(self.cwd))
            {
                if !visited.insert(parent.to_path_buf()) {
                    continue;
                }
                let config = parent.join("conftest.py");
                if config.is_file() && !files.contains_key(&config) {
                    files.insert(config, None);
                }
            }
        }
        files
            .into_iter()
            .filter_map(|(path, names)| {
                collect_file(
                    &path,
                    self.cwd,
                    &self.settings,
                    names.as_deref().unwrap_or_default(),
                )
                .transpose()
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(Into::into)
    }

    /// Walks a directory concurrently while serializing package mutation on a receiver thread.
    fn collect_directory(&self, path: &Utf8PathBuf) -> Result<CollectedPackage> {
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

                match collect_file_for_scheduling(&file_path, self.cwd, &self.settings, &[]) {
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

    /// Collects explicit files, functions, and directories into one package tree.
    pub(super) fn collect_all(&self, test_paths: Vec<TestPath>) -> Result<CollectedPackage> {
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
                    parametrize_index: _,
                }) => {
                    if let Some(module) = collect_file_for_scheduling(
                        &file_path,
                        self.cwd,
                        &self.settings,
                        &[function_name],
                    )? {
                        session_package.add_module(module);
                    }
                }
                TestPath::File(file_path) => {
                    if let Some(module) =
                        collect_file_for_scheduling(&file_path, self.cwd, &self.settings, &[])?
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
    use indoc::indoc;

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
            collect_doctests: false,
        }
    }

    #[test]
    fn scheduling_collection_discards_module_context() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let root = temp_path(&temp_dir);
        let test_path = root.join("test_sample.py");
        std::fs::write(
            &test_path,
            indoc! {r#"
                """Example.

                >>> 1 + 1
                2
                """
                value = 1
                def test_sample(): assert value == 1
            "#},
        )
        .expect("write test file");
        let mut settings = collection_settings();
        settings.collect_doctests = true;

        let module = collect_file_for_scheduling(&test_path, root, &settings, &[])
            .expect("collect file")
            .expect("module should collect");

        assert_eq!(module.test_function_defs[0].name.as_str(), "test_sample");
        assert_eq!(module.doctests[0].name, "doctest:@module");
        assert_eq!(module.source_text, "");
        assert!(module.module_body.is_empty());
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
