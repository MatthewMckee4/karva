use camino::{Utf8Path, Utf8PathBuf};

mod function_kind;
mod name;

pub use function_kind::FunctionKind;
pub use name::{ModulePath, QualifiedFunctionName, QualifiedTestName};
use pyo3::Python;
use ruff_python_ast::{Expr, PythonVersion, StmtFunctionDef};

/// Check whether a file path has a `.py` extension.
pub fn is_python_file(path: &Utf8Path) -> bool {
    path.extension().is_some_and(|extension| extension == "py")
}

/// Check if a function definition has a @fixture decorator
pub fn is_fixture_function(val: &StmtFunctionDef) -> bool {
    val.decorator_list
        .iter()
        .any(|decorator| is_fixture(&decorator.expression))
}

/// Check whether an expression resolves to a fixture reference.
pub fn is_fixture(expr: &Expr) -> bool {
    match expr {
        Expr::Name(name) => name.id == "fixture",
        Expr::Attribute(attr) => attr.attr.id == "fixture",
        Expr::Call(call) => is_fixture(call.func.as_ref()),
        _ => false,
    }
}

/// Gets the module name from a path.
///
/// This can return None if the path is not relative to the current working directory.
pub fn module_name(cwd: &Utf8PathBuf, path: &Utf8Path) -> Option<String> {
    let relative_path = path.strip_prefix(cwd).ok()?;

    let components: Vec<_> = relative_path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();

    Some(components.join(".").trim_end_matches(".py").to_string())
}

/// Retrieves the current Python interpreter version.
///
/// This function queries the embedded Python interpreter to determine
/// the major and minor version numbers, which are used for AST parsing
/// compatibility and feature detection.
pub fn current_python_version() -> PythonVersion {
    Python::initialize();
    PythonVersion::from(Python::attach(|py| {
        let version_info = py.version_info();
        (version_info.major, version_info.minor)
    }))
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;

    use super::*;

    #[cfg(unix)]
    #[test]
    fn test_module_name() {
        assert_eq!(
            module_name(&Utf8PathBuf::from("/"), &Utf8PathBuf::from("/test.py")),
            Some("test".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_module_name_with_directory() {
        assert_eq!(
            module_name(
                &Utf8PathBuf::from("/"),
                &Utf8PathBuf::from("/test_dir/test.py")
            ),
            Some("test_dir.test".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_module_name_with_gitignore() {
        assert_eq!(
            module_name(
                &Utf8PathBuf::from("/"),
                &Utf8PathBuf::from("/tests/test.py")
            ),
            Some("tests.test".to_string())
        );
    }

    #[cfg(unix)]
    mod unix_tests {
        use super::*;

        #[test]
        fn test_unix_paths() {
            assert_eq!(
                module_name(
                    &Utf8PathBuf::from("/home/user/project"),
                    &Utf8PathBuf::from("/home/user/project/src/module/test.py")
                ),
                Some("src.module.test".to_string())
            );
        }
    }

    #[cfg(windows)]
    mod windows_tests {
        use super::*;

        #[test]
        fn test_windows_paths() {
            assert_eq!(
                module_name(
                    &Utf8PathBuf::from("C:\\Users\\user\\project"),
                    &Utf8PathBuf::from("C:\\Users\\user\\project\\src\\module\\test.py")
                ),
                Some("src.module.test".to_string())
            );
        }
    }

    #[test]
    fn test_is_python_file_with_py_extension() {
        assert!(is_python_file(Utf8Path::new("test.py")));
    }

    #[test]
    fn test_is_python_file_with_non_py_extension() {
        assert!(!is_python_file(Utf8Path::new("test.txt")));
    }

    #[test]
    fn test_is_python_file_with_no_extension() {
        assert!(!is_python_file(Utf8Path::new("test")));
    }

    #[cfg(unix)]
    #[test]
    fn test_module_name_not_relative() {
        assert_eq!(
            module_name(
                &Utf8PathBuf::from("/home/user/project"),
                &Utf8PathBuf::from("/other/path/test.py")
            ),
            None
        );
    }
}
