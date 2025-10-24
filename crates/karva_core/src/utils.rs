use karva_project::{
    path::SystemPathBuf,
    project::{Project, ProjectOptions},
};
use pyo3::{PyResult, Python, prelude::*, types::PyAnyMethods};
use ruff_python_ast::PythonVersion;

/// Retrieves the current Python interpreter version.
///
/// This function queries the embedded Python interpreter to determine
/// the major and minor version numbers, which are used for AST parsing
/// compatibility and feature detection.
#[must_use]
pub fn current_python_version() -> PythonVersion {
    PythonVersion::from(Python::attach(|py| {
        let version_info = py.version_info();
        (version_info.major, version_info.minor)
    }))
}

/// Adds a directory path to Python's sys.path for module resolution.
///
/// This is essential for allowing Python to import modules from the test
/// directories. The path is inserted at the specified index to control
/// import precedence.
///
/// # Arguments
/// * `py` - Python interpreter instance
/// * `path` - Directory path to add to sys.path
/// * `index` - Position to insert the path (0 for highest priority)
pub(crate) fn add_to_sys_path(py: Python<'_>, path: &SystemPathBuf, index: isize) -> PyResult<()> {
    let sys_module = py.import("sys")?;
    let sys_path = sys_module.getattr("path")?;
    sys_path.call_method1("insert", (index, path.display().to_string()))?;
    Ok(())
}

/// Trait for converting types to more general trait objects.
///
/// This trait is primarily used for converting collections of concrete types
/// to collections of trait objects, enabling polymorphic behavior while
/// maintaining type safety.
pub(crate) trait Upcast<T> {
    fn upcast(self) -> T;
}

/// Identity implementation - any type can upcast to itself
impl<T> Upcast<T> for T {
    fn upcast(self) -> T {
        self
    }
}

/// Redirects Python's stdout and stderr to /dev/null if output is disabled.
///
/// This function is used to suppress Python output during test execution
/// when the user hasn't requested to see it. It returns a handle to the
/// null file for later restoration.
fn redirect_python_output<'py>(
    py: Python<'py>,
    options: &ProjectOptions,
) -> PyResult<Option<Bound<'py, PyAny>>> {
    if options.show_output() {
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

/// Executes a function with Python GIL, managing output redirection.
///
/// This wrapper function handles the complexity of Python output management
/// during test execution. It automatically redirects output if configured
/// and ensures proper cleanup regardless of whether the function succeeds or fails.
///
/// # Arguments
/// * `project` - Project configuration containing output preferences
/// * `f` - Function to execute with Python access
pub(crate) fn attach<F, R>(project: &Project, f: F) -> R
where
    F: for<'py> FnOnce(Python<'py>) -> R,
{
    Python::attach(|py| {
        let null_file = redirect_python_output(py, project.options());
        let result = f(py);
        if let Ok(Some(null_file)) = null_file {
            let _ = restore_python_output(py, &null_file);
        }
        result
    })
}

/// Creates an iterator that yields each item with all items above it in the hierarchy.
///
/// This function is used for fixture dependency resolution where each scope needs
/// access to all parent scopes. For example, given [session, package, module],
/// it yields: (module, [session, package]), (package, [session]), (session, []).
///
/// # Arguments
/// * `items` - Slice of references representing the scope hierarchy
///
/// # Returns
/// Iterator yielding tuples of (`current_item`, ancestors)
pub(crate) fn create_hierarchy_iterator<'a, T>(
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

#[cfg(test)]
mod tests {
    use super::*;

    mod python_version_tests {
        use super::*;

        #[test]
        fn test_current_python_version() {
            let version = current_python_version();
            assert!(version >= PythonVersion::from((3, 7)));
        }
    }
}
