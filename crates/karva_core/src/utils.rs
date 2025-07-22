use karva_project::{
    path::SystemPathBuf,
    project::{Project, ProjectOptions},
};
use pyo3::{PyResult, Python, prelude::*, types::PyAnyMethods};
use ruff_python_ast::PythonVersion;

#[must_use]
pub fn current_python_version() -> PythonVersion {
    PythonVersion::from(Python::with_gil(|py| {
        let inferred_python_version = py.version_info();
        (inferred_python_version.major, inferred_python_version.minor)
    }))
}

// Add the path to the python sys.path list at the given index
pub(crate) fn add_to_sys_path(py: Python<'_>, path: &SystemPathBuf, index: isize) -> PyResult<()> {
    let sys_path = py.import("sys")?;
    let sys_path = sys_path.getattr("path")?;
    sys_path.call_method1("insert", (index, path.display().to_string()))?;
    Ok(())
}

pub(crate) trait Upcast<T> {
    fn upcast(self) -> T;
}

impl<T> Upcast<T> for T {
    fn upcast(self) -> T {
        self
    }
}

fn redirect_output<'py>(
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

fn restore_output<'py>(py: Python<'py>, null_file: &Bound<'py, PyAny>) -> PyResult<()> {
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

pub(crate) fn with_gil<F, R>(project: &Project, f: F) -> R
where
    F: for<'py> FnOnce(Python<'py>) -> R,
{
    Python::with_gil(|py| {
        let null_file = redirect_output(py, project.options());
        let result = f(py);
        if let Ok(Some(null_file)) = null_file {
            let _ = restore_output(py, &null_file);
        }
        result
    })
}

pub(crate) fn partition_iter<'a, T>(items: &[&'a T]) -> impl Iterator<Item = (&'a T, Vec<&'a T>)> {
    let mut parents_above_current_parent = items.to_vec();
    let mut i = items.len();
    std::iter::from_fn(move || {
        if i > 0 {
            i -= 1;
            let parent = items[i];
            parents_above_current_parent.truncate(i);
            Some((parent, parents_above_current_parent.clone()))
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
