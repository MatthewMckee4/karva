use std::{sync::Arc, thread};

use camino::{Utf8Path, Utf8PathBuf};
use crossbeam_channel::unbounded;
use ignore::{WalkBuilder, WalkState};
use karva_project::TestPath;
use ruff_python_ast::Stmt;
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};

use super::models::{CollectedModule, CollectedPackage};
use crate::{
    Context, collection::ModuleType, diagnostic::report_invalid_path,
    extensions::fixtures::is_fixture_function, name::ModulePath,
};

pub struct ParallelCollector<'ctx, 'proj, 'rep> {
    context: &'ctx Context<'proj, 'rep>,
}

impl<'ctx, 'proj, 'rep> ParallelCollector<'ctx, 'proj, 'rep> {
    pub const fn new(context: &'ctx Context<'proj, 'rep>) -> Self {
        Self { context }
    }

    /// Collect from a directory in parallel using `WalkParallel`.
    pub(crate) fn collect_directory(&self, path: &Utf8PathBuf) -> CollectedPackage {
        // Create channels for communication
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

                if let Some(module) = collect_file(&file_path, self.context, None) {
                    let _ = tx.send(module);
                }

                WalkState::Continue
            })
        });

        // Drop the original sender to close the channel
        drop(tx);

        receiver_handle.join().unwrap()
    }

    /// Collect from all paths and build a complete package structure.
    pub(crate) fn collect_all(&self) -> CollectedPackage {
        let mut session_package = CollectedPackage::new(self.context.project().cwd().clone());

        let test_paths = self.context.project().test_paths();

        tracing::info!("Collecting test paths: {:?}", test_paths);

        for path_result in test_paths {
            let path = match path_result {
                Ok(path) => path,
                Err(error) => {
                    report_invalid_path(self.context, &error);
                    continue;
                }
            };

            let path_for_config = path.path().to_owned();

            match path {
                TestPath::Directory(dir_path) => {
                    let package = self.collect_directory(&dir_path);
                    session_package.add_package(package);
                }
                TestPath::Function {
                    path: file_path,
                    function_name,
                } => {
                    if let Some(module) =
                        collect_file(&file_path, self.context, Some(&function_name))
                    {
                        session_package.add_module(module);
                    }
                }
                TestPath::File(file_path) => {
                    if let Some(module) = collect_file(&file_path, self.context, None) {
                        session_package.add_module(module);
                    }
                }
            }

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

                if let Some(module) = collect_file(&conftest_path, self.context, None) {
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
        let num_threads = karva_project::max_parallelism().get();

        WalkBuilder::new(path)
            .threads(num_threads)
            .standard_filters(true)
            .require_git(false)
            .git_global(false)
            .parents(true)
            .follow_links(true)
            .git_ignore(self.context.project().settings().src().respect_ignore_files)
            .types({
                let mut types = ignore::types::TypesBuilder::new();
                types.add("python", "*.py").unwrap();
                types.select("python");
                types.build().unwrap()
            })
            .build_parallel()
    }
}

fn collect_file(
    path: &Utf8PathBuf,
    context: &Context,
    only_function_name: Option<&str>,
) -> Option<CollectedModule> {
    let module_path = ModulePath::new(path, context.project().cwd())?;

    let source_text = context.db().system().read_to_string(path).ok()?;

    let module_type: ModuleType = path.into();

    let parsed = parse_unchecked(
        &source_text,
        ParseOptions::from(Mode::Module)
            .with_target_version(context.project().metadata().python_version()),
    )
    .try_into_module()?;

    let mut collected_module = CollectedModule::new(module_path, module_type, source_text);

    for stmt in parsed.into_syntax().body {
        if let Stmt::FunctionDef(function_def) = stmt {
            if let Some(only_function_name) = only_function_name {
                if function_def.name.as_str() != only_function_name {
                    continue;
                }
            }
            if is_fixture_function(&function_def) {
                collected_module.add_fixture_function_def(Arc::new(function_def));
            } else if function_def
                .name
                .to_string()
                .starts_with(&context.project().settings().test().test_function_prefix)
            {
                collected_module.add_test_function_def(Arc::new(function_def));
            }
        }
    }

    Some(collected_module)
}
