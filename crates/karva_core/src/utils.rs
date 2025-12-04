use std::{collections::HashMap, fmt::Write};

use camino::Utf8Path;
use karva_project::{Db, ProjectSettings, System};
use pyo3::{PyResult, Python, prelude::*, types::PyAnyMethods};
use ruff_python_ast::PythonVersion;
use ruff_source_file::{SourceFile, SourceFileBuilder};

/// Retrieves the current Python interpreter version.
///
/// This function queries the embedded Python interpreter to determine
/// the major and minor version numbers, which are used for AST parsing
/// compatibility and feature detection.
pub fn current_python_version() -> PythonVersion {
    PythonVersion::from(attach(|py| {
        let version_info = py.version_info();
        (version_info.major, version_info.minor)
    }))
}

/// Get the source file for the given utf8 path.
pub(crate) fn source_file(system: &dyn System, path: &Utf8Path) -> SourceFile {
    SourceFileBuilder::new(
        path.as_str(),
        system
            .read_to_string(path)
            .expect("Failed to read source file"),
    )
    .finish()
}

/// Adds a directory path to Python's sys.path at the specified index.
pub(crate) fn add_to_sys_path(py: Python<'_>, path: &Utf8Path, index: isize) -> PyResult<()> {
    let sys_module = py.import("sys")?;
    let sys_path = sys_module.getattr("path")?;
    sys_path.call_method1("insert", (index, path.to_string()))?;
    Ok(())
}

/// Redirects Python's stdout and stderr to /dev/null if output is disabled.
///
/// This function is used to suppress Python output during test execution
/// when the user hasn't requested to see it. It returns a handle to the
/// null file for later restoration.
fn redirect_python_output<'py>(
    py: Python<'py>,
    settings: &ProjectSettings,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    if settings.terminal().show_python_output {
        Ok(None)
    } else {
        let sys = py.import("sys")?;
        let os = py.import("os")?;
        let builtins = py.import("builtins")?;
        let logging = py.import("logging")?;

        let devnull = os.getattr("devnull")?;
        let open_file_function = builtins.getattr("open")?;
        let null_file = open_file_function.call1((devnull, "w"))?;

        for output in ["stdout", "stderr"] {
            sys.setattr(output, null_file.clone())?;
        }

        logging.call_method1("disable", (logging.getattr("CRITICAL")?,))?;

        Ok(Some(null_file))
    }
}

/// Restores Python's stdout and stderr from the null file redirect.
///
/// This function cleans up the output redirection by closing the null file
/// handles and restoring normal output streams.
fn restore_python_output<'py>(py: Python<'py>, null_file: &Bound<'py, PyAny>) -> PyResult<()> {
    let sys = py.import("sys")?;
    let logging = py.import("logging")?;

    for output in ["stdout", "stderr"] {
        let current_output = sys.getattr(output)?;
        let close_method = current_output.getattr("close")?;
        close_method.call0()?;
        sys.setattr(output, null_file.clone())?;
    }

    logging.call_method1("disable", (logging.getattr("CRITICAL")?,))?;
    Ok(())
}

/// A wrapper around `Python::attach` so we can manage the stdout and stderr redirection.
pub(crate) fn attach_with_project<F, R>(db: &dyn Db, f: F) -> R
where
    F: for<'py> FnOnce(Python<'py>) -> R,
{
    attach(|py| {
        let null_file = redirect_python_output(py, db.project().settings());
        let result = f(py);
        if let Ok(Some(null_file)) = null_file {
            let _ = restore_python_output(py, &null_file);
        }
        result
    })
}

/// A simple wrapper around `Python::attach` that initializes the Python interpreter first.
pub(crate) fn attach<F, R>(f: F) -> R
where
    F: for<'py> FnOnce(Python<'py>) -> R,
{
    Python::initialize();
    Python::attach(f)
}

/// Creates an iterator that yields each item with all items after it.
///
/// For example, given [session, package, module],
/// it yields: (module, [session, package]), (package, [session]), (session, []).
pub(crate) fn iter_with_ancestors<'a, T: ?Sized>(
    items: &[&'a T],
) -> impl Iterator<Item = (&'a T, Vec<&'a T>)> {
    let mut ancestors = items.to_vec();
    let mut current_index = items.len();

    std::iter::from_fn(move || {
        if current_index > 0 {
            current_index -= 1;
            let current_item = items[current_index];
            ancestors.truncate(current_index);
            Some((current_item, ancestors.clone()))
        } else {
            None
        }
    })
}

pub(crate) fn full_test_name(
    py: Python,
    function: String,
    kwargs: &HashMap<String, Py<PyAny>>,
) -> String {
    if kwargs.is_empty() {
        function
    } else {
        let mut args_str = String::new();
        let mut sorted_kwargs: Vec<_> = kwargs.iter().collect();
        sorted_kwargs.sort_by_key(|(key, _)| &**key);

        for (i, (key, value)) in sorted_kwargs.iter().enumerate() {
            if i > 0 {
                args_str.push_str(", ");
            }
            if let Ok(value) = value.cast_bound::<PyAny>(py) {
                let trimmed_value_str = truncate_string(&value.to_string());
                let truncated_key = truncate_string(key);
                let _ = write!(args_str, "{truncated_key}={trimmed_value_str}");
            }
        }
        format!("{function}({args_str})")
    }
}

const TRUNCATE_LENGTH: usize = 30;

pub(crate) fn truncate_string(value: &str) -> String {
    if value.chars().count() > TRUNCATE_LENGTH {
        let truncated: String = value.chars().take(TRUNCATE_LENGTH - 3).collect();
        format!("{truncated}...")
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod python_version_tests {
        use super::*;

        #[test]
        fn test_current_python_version() {
            let version = current_python_version();
            assert!(version >= PythonVersion::PY37);
        }
    }

    mod utils_tests {
        use super::*;

        #[test]
        fn test_iter_with_ancestors() {
            let items = vec!["session", "package", "module"];
            let expected = vec![
                ("module", vec!["session", "package"]),
                ("package", vec!["session"]),
                ("session", vec![]),
            ];
            let result: Vec<(&str, Vec<&str>)> = iter_with_ancestors(&items).collect();
            assert_eq!(result, expected);
        }
    }
}
