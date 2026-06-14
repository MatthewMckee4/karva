use camino::{Utf8Path, Utf8PathBuf};
use ruff_python_ast::{PythonVersion, Stmt};
use ruff_python_parser::{Mode, ParseOptions, parse_unchecked};
use thiserror::Error;

use karva_python_semantic::ModulePath;
use karva_python_semantic::is_fixture_function;

mod models;

pub use models::{CollectedModule, CollectedPackage, ModuleType};

#[derive(Debug, Error)]
pub enum CollectionError {
    #[error("failed to read Python source file `{path}`: {source}")]
    ReadSource {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
}

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
    cwd: &Utf8Path,
    settings: &CollectionSettings,
    function_names: &[String],
) -> Result<Option<CollectedModule>, CollectionError> {
    let Some(module_path) = ModulePath::new(path, &cwd.to_path_buf()) else {
        return Ok(None);
    };

    let source_text =
        std::fs::read_to_string(path).map_err(|source| CollectionError::ReadSource {
            path: path.clone(),
            source,
        })?;

    let module_type: ModuleType = path.into();

    let mut parse_options = ParseOptions::from(Mode::Module);

    parse_options = parse_options.with_target_version(settings.python_version);

    let Some(parsed) = parse_unchecked(&source_text, parse_options).try_into_module() else {
        return Ok(None);
    };

    let mut collected_module = CollectedModule::new(module_path, module_type, source_text);

    for stmt in parsed.into_syntax().body {
        if let Stmt::FunctionDef(function_def) = stmt {
            if settings.collect_fixtures && is_fixture_function(&function_def) {
                collected_module.add_fixture_function_def(function_def);
                continue;
            }

            if is_test_function_to_collect(
                &function_def.name,
                function_names,
                settings.test_function_prefix,
            ) {
                collected_module.add_test_function_def(function_def);
            }
        }
    }

    Ok(Some(collected_module))
}

/// Returns `true` if a function should be collected as a test.
///
/// When `explicit_names` is empty, any function whose name starts with
/// `prefix` is considered a test. When `explicit_names` is provided,
/// only functions whose name appears in the list are collected.
fn is_test_function_to_collect(name: &str, explicit_names: &[String], prefix: &str) -> bool {
    if explicit_names.is_empty() {
        name.starts_with(prefix)
    } else {
        explicit_names.iter().any(|n| n == name)
    }
}

#[cfg(test)]
mod tests {
    use ruff_python_ast::PythonVersion;

    use super::*;

    fn settings() -> CollectionSettings<'static> {
        CollectionSettings {
            python_version: PythonVersion::PY312,
            test_function_prefix: "test_",
            respect_ignore_files: true,
            collect_fixtures: false,
        }
    }

    #[test]
    fn collect_file_reports_read_errors() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let cwd = Utf8Path::from_path(temp_dir.path()).expect("temp dir should be UTF-8");
        let path = cwd.join("test_unreadable.py");
        std::fs::create_dir(&path).expect("create directory at Python file path");

        let error = collect_file(&path, cwd, &settings(), &[]).expect_err("read should fail");

        assert!(matches!(
            error,
            CollectionError::ReadSource { path: error_path, .. } if error_path == path
        ));
    }
}
