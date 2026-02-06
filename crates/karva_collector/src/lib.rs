use camino::Utf8PathBuf;
use ruff_python_ast::{PythonVersion, Stmt};
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};

use karva_python_semantic::ModulePath;
use karva_python_semantic::is_fixture_function;
use karva_system::System;

mod models;

pub use models::{CollectedModule, CollectedPackage, ModuleType};

/// Settings that control how test files are collected and parsed.
pub struct CollectionSettings<'a> {
    /// The Python version to use when parsing source files.
    pub python_version: PythonVersion,
    /// The prefix used to identify test functions (e.g., `"test_"`).
    pub test_function_prefix: &'a str,
    /// Whether to respect `.gitignore` and similar ignore files during file discovery.
    pub respect_ignore_files: bool,
    /// Whether to collect fixture function definitions in addition to test functions.
    pub collect_fixtures: bool,
}

/// Collects test functions and fixtures from a Python file.
///
/// If `function_names` is empty, all test functions matching the configured prefix are collected.
/// If `function_names` is non-empty, only test functions with names in the list are collected.
/// Fixtures are always collected regardless of the filter.
pub fn collect_file(
    path: &Utf8PathBuf,
    system: &dyn System,
    settings: &CollectionSettings,
    function_names: &[String],
) -> Option<CollectedModule> {
    let module_path = ModulePath::new(path, &system.current_directory().to_path_buf())?;

    let source_text = system.read_to_string(path).ok()?;

    let module_type: ModuleType = path.into();

    let mut parse_options = ParseOptions::from(Mode::Module);

    parse_options = parse_options.with_target_version(settings.python_version);

    let parsed = parse_unchecked(&source_text, parse_options).try_into_module()?;

    let mut collected_module = CollectedModule::new(module_path, module_type, source_text);

    for stmt in parsed.into_syntax().body {
        if let Stmt::FunctionDef(function_def) = stmt {
            if settings.collect_fixtures && is_fixture_function(&function_def) {
                collected_module.add_fixture_function_def(function_def);
                continue;
            }

            if function_names.is_empty() {
                if function_def.name.starts_with(settings.test_function_prefix) {
                    collected_module.add_test_function_def(function_def);
                }
            } else if function_names.iter().any(|name| function_def.name == *name) {
                collected_module.add_test_function_def(function_def);
            }
        }
    }

    Some(collected_module)
}
