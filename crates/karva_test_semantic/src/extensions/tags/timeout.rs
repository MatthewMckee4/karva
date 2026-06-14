use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyTuple;

/// Represents a per-test timeout limit, in seconds.
///
/// Enforcement is performed by `run_test_with_timeout` in `utils.rs`:
/// sync tests run in a `ThreadPoolExecutor` worker, async tests are wrapped
/// in `asyncio.wait_for`.
#[derive(Debug, Clone, Copy)]
pub struct TimeoutTag {
    seconds: f64,
}

impl TimeoutTag {
    pub(crate) fn new(seconds: f64) -> Self {
        Self { seconds }
    }

    pub(crate) fn seconds(self) -> f64 {
        self.seconds
    }

    /// Parse `@pytest.mark.timeout(seconds)`.
    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> PyResult<Option<Self>> {
        let args = py_mark.getattr("args")?;
        let tuple = args.extract::<Bound<'_, PyTuple>>()?;
        if tuple.is_empty() {
            return Err(PyValueError::new_err(
                "pytest timeout mark requires a seconds argument",
            ));
        }

        let first = tuple.get_item(0)?;
        let seconds = first.extract::<f64>().map_err(|_| {
            PyValueError::new_err("pytest timeout mark seconds must be a finite, positive number")
        })?;
        if !(seconds.is_finite() && seconds > 0.0) {
            return Err(PyValueError::new_err(
                "pytest timeout mark seconds must be a finite, positive number",
            ));
        }
        Ok(Some(Self { seconds }))
    }
}
