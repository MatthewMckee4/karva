use pyo3::Python;
use ruff_python_ast::PythonVersion;

#[must_use]
pub fn current_python_version() -> PythonVersion {
    PythonVersion::from(Python::with_gil(|py| {
        let inferred_python_version = py.version_info();
        (inferred_python_version.major, inferred_python_version.minor)
    }))
}
