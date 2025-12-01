use pyo3::prelude::*;
pub use python::Param;

pub mod python;

// SkipError exception that can be raised to skip tests at runtime with an optional reason
pyo3::create_exception!(karva, SkipError, pyo3::exceptions::PyException);

// FailError exception that can be raised to fail tests at runtime with an optional reason
pyo3::create_exception!(karva, FailError, pyo3::exceptions::PyException);

/// Skip the current test at runtime with an optional reason.
///
/// This function raises a `SkipError` exception which will be caught by the test runner
/// and mark the test as skipped.
#[pyfunction]
#[pyo3(signature = (reason = None))]
pub fn skip(_py: Python<'_>, reason: Option<String>) -> PyResult<()> {
    let message = reason.unwrap_or_default();
    Err(SkipError::new_err(message))
}

/// Fail the current test at runtime with an optional reason.
///
/// This function raises a `FailError` exception which will be caught by the test runner
/// and mark the test as failed with the given reason.
#[pyfunction]
#[pyo3(signature = (reason = None))]
pub fn fail(_py: Python<'_>, reason: Option<String>) -> PyResult<()> {
    Err(FailError::new_err(reason))
}

#[pyfunction]
#[pyo3(signature = (*values, tags = None))]
pub fn param(
    py: Python<'_>,
    values: Vec<Py<PyAny>>,
    tags: Option<Vec<Py<PyAny>>>,
) -> PyResult<Param> {
    Param::new(py, values, tags.unwrap_or_default())
}
