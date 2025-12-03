use std::{sync::Arc, thread};

use camino::{Utf8Path, Utf8PathBuf};
use crossbeam_channel::unbounded;
use ignore::WalkBuilder;
use ruff_python_ast::Stmt;
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};

use super::models::{CollectedModule, CollectedPackage};
use crate::{
    Context, diagnostic::report_invalid_path, discovery::ModuleType,
    extensions::fixtures::is_fixture_function, name::ModulePath,
};

pub struct ParallelCollector<'ctx, 'proj, 'rep> {
    context: &'ctx Context<'proj, 'rep>,
}

impl<'ctx, 'proj, 'rep> ParallelCollector<'ctx, 'proj, 'rep> {
    pub const fn new(context: &'ctx Context<'proj, 'rep>) -> Self {
        Self { context }
    }

    /// Collect all function definitions from a single file.
    fn collect_file(&self, path: &Utf8PathBuf) -> Option<CollectedModule> {
        Self::collect_file_static(
            path,
            self.context.project().cwd(),
            self.context.project().metadata().python_version(),
            self.context.project().options().test_prefix(),
        )
    }

    /// Collect from a single test file path.
    pub(crate) fn collect_test_file(&self, path: &Utf8PathBuf) -> Option<CollectedModule> {
        self.collect_file(path)
    }

    /// Collect from a directory in parallel using `WalkParallel`.
    pub(crate) fn collect_directory(&self, path: &Utf8PathBuf) -> CollectedPackage {
        let mut package = CollectedPackage::new(path.clone());

        // Create channels for communication
        let (tx, rx) = unbounded();

        // Spawn receiver thread to collect results
        let receiver_handle = thread::spawn(move || {
            let mut modules = Vec::new();
            for module in rx {
                modules.push(module);
            }
            modules
        });

        // Walk directory in parallel and process files
        let walker = self.create_parallel_walker(path);
        let cwd = self.context.project().cwd().clone();
        let python_version = self.context.project().metadata().python_version();
        let test_prefix = self.context.project().options().test_prefix().to_string();

        walker.run(|| {
            let tx = tx.clone();
            let cwd = cwd.clone();
            let test_prefix = test_prefix.clone();

            Box::new(move |entry| {
                let Ok(entry) = entry else {
                    return ignore::WalkState::Continue;
                };

                // Only process files
                if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                    return ignore::WalkState::Continue;
                }

                let Ok(file_path) = Utf8PathBuf::from_path_buf(entry.path().to_path_buf()) else {
                    return ignore::WalkState::Continue;
                };

                // Collect the module
                if let Some(module) =
                    Self::collect_file_static(&file_path, &cwd, python_version, &test_prefix)
                {
                    let _ = tx.send(module);
                }

                ignore::WalkState::Continue
            })
        });

        // Drop the sender so receiver knows we're done
        drop(tx);

        // Wait for receiver to finish
        let modules = receiver_handle.join().unwrap();

        // Add all collected modules to the package
        for collected_module in modules {
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
    }

    /// Static version of `collect_file` that doesn't need self reference.
    /// Used for parallel processing where we can't share &self across threads.
    fn collect_file_static(
        path: &Utf8PathBuf,
        cwd: &Utf8PathBuf,
        python_version: ruff_python_ast::PythonVersion,
        test_prefix: &str,
    ) -> Option<CollectedModule> {
        let module_path = ModulePath::new(path, cwd)?;

        let source_text = std::fs::read_to_string(path).ok()?;

        let module_type: ModuleType = path.into();

        // Parse the file to collect function definitions
        let parsed = parse_unchecked(
            &source_text,
            ParseOptions::from(Mode::Module).with_target_version(python_version),
        )
        .try_into_module()?;

        // Collect and categorize top-level function definitions from the parsed AST
        let mut test_defs = Vec::new();
        let mut fixture_defs = Vec::new();

        for stmt in parsed.into_syntax().body {
            if let Stmt::FunctionDef(function_def) = stmt {
                // Check if it's a fixture function
                if is_fixture_function(&function_def) {
                    fixture_defs.push(Arc::new(function_def));
                }
                // Check if it's a test function (starts with test prefix)
                else if function_def.name.to_string().starts_with(test_prefix) {
                    test_defs.push(Arc::new(function_def));
                }
                // Otherwise, ignore the function (it's not relevant for testing)
            }
        }

        let mut collected_module =
            CollectedModule::new(module_path, path.clone(), module_type, source_text);

        // Add collected functions
        for test_def in test_defs {
            collected_module.add_test_function_def(test_def);
        }
        for fixture_def in fixture_defs {
            collected_module.add_fixture_function_def(fixture_def);
        }

        Some(collected_module)
    }

    /// Collect from all paths and build a complete package structure.
    pub(crate) fn collect_all(&self) -> CollectedPackage {
        let mut session_package = CollectedPackage::new(self.context.project().cwd().clone());

        // Process all test paths
        for path_result in self.context.project().test_paths() {
            let Ok(path) = path_result else {
                // Report invalid path errors
                if let Err(error) = path_result {
                    report_invalid_path(self.context, &error);
                }
                continue;
            };

            // Clone the path for parent configuration lookup
            let path_for_config = path.path().to_owned();

            match path {
                karva_project::TestPath::File(file_path) => {
                    if let Some(module) = self.collect_test_file(&file_path) {
                        session_package.add_module(module);
                    }
                }
                karva_project::TestPath::Directory(dir_path) => {
                    let package = self.collect_directory(&dir_path);
                    session_package.add_package(package);
                }
                karva_project::TestPath::Function {
                    path: file_path, ..
                } => {
                    // For specific function paths, still collect the whole module
                    if let Some(module) = self.collect_test_file(&file_path) {
                        session_package.add_module(module);
                    }
                }
            }

            // Collect parent configuration files
            self.collect_parent_configuration(&path_for_config, &mut session_package);
        }

        session_package.shrink();
        session_package
    }

    /// Collect parent configuration files (conftest.py).
    fn collect_parent_configuration(
        &self,
        path: &Utf8Path,
        session_package: &mut CollectedPackage,
    ) {
        let mut current_path = if path.is_dir() {
            path
        } else {
            match path.parent() {
                Some(parent) => parent,
                None => return,
            }
        };

        loop {
            let conftest_path = current_path.join("conftest.py");
            if conftest_path.exists() {
                let mut package = CollectedPackage::new(current_path.to_path_buf());

                if let Some(module) = self.collect_test_file(&conftest_path) {
                    package.add_configuration_module(module);
                    session_package.add_package(package);
                }
            }

            if current_path == self.context.project().cwd() {
                break;
            }

            current_path = match current_path.parent() {
                Some(parent) => parent,
                None => break,
            };
        }
    }

    /// Creates a configured parallel directory walker for Python file discovery.
    fn create_parallel_walker(&self, path: &Utf8PathBuf) -> ignore::WalkParallel {
        // Configure thread pool size for optimal parallelism
        let num_threads = karva_project::max_parallelism().get();

        WalkBuilder::new(path)
            .threads(num_threads)
            .standard_filters(true)
            .require_git(false)
            .git_global(false)
            .parents(true)
            .git_ignore(!self.context.project().options().no_ignore())
            .types({
                let mut types = ignore::types::TypesBuilder::new();
                types.add("python", "*.py").unwrap();
                types.select("python");
                types.build().unwrap()
            })
            .build_parallel()
    }
}
