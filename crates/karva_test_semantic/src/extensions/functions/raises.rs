use pyo3::prelude::*;
use pyo3::types::PyType;

use super::FailError;

#[pyclass]
pub struct ExceptionInfo {
    #[pyo3(get, name = "type")]
    pub exc_type: Option<Py<PyAny>>,

    #[pyo3(get)]
    pub value: Option<Py<PyAny>>,

    #[pyo3(get)]
    pub tb: Option<Py<PyAny>>,
}

#[pymethods]
impl ExceptionInfo {
    #[new]
    fn new() -> Self {
        Self {
            exc_type: None,
            value: None,
            tb: None,
        }
    }
}

#[pyclass]
pub struct RaisesContext {
    expected_exception: Py<PyAny>,
    match_pattern: Option<String>,
    exc_info: Py<ExceptionInfo>,
}

#[pymethods]
impl RaisesContext {
    fn __enter__(&self, py: Python<'_>) -> Py<ExceptionInfo> {
        self.exc_info.clone_ref(py)
    }

    fn __exit__(
        &self,
        py: Python<'_>,
        exc_type: Option<Py<PyAny>>,
        exc_val: Option<Py<PyAny>>,
        exc_tb: Option<Py<PyAny>>,
    ) -> PyResult<bool> {
        let Some(exc_type_obj) = exc_type else {
            let repr = self.expected_exception.bind(py).repr()?.to_string();
            return Err(FailError::new_err(format!("DID NOT RAISE {repr}")));
        };

        let exc_type_bound = exc_type_obj.bind(py);
        let expected_bound = self.expected_exception.bind(py);

        let exc_py_type = exc_type_bound.cast::<PyType>()?;
        let expected_py_type = expected_bound.cast::<PyType>()?;

        if !exc_py_type.is_subclass(expected_py_type)? {
            return Ok(false);
        }

        if let Some(ref pattern) = self.match_pattern {
            let exc_str = if let Some(ref val) = exc_val {
                val.bind(py).str()?.to_string()
            } else {
                String::new()
            };

            let re_module = py.import("re")?;
            let result = re_module.call_method1("search", (pattern.as_str(), exc_str.as_str()))?;

            if result.is_none() {
                return Err(FailError::new_err(format!(
                    "Raised exception did not match pattern '{pattern}'"
                )));
            }
        }

        let mut info = self.exc_info.borrow_mut(py);
        info.exc_type = Some(exc_type_obj);
        info.value = exc_val;
        info.tb = exc_tb;

        Ok(true)
    }
}

/// Assert that a block of code raises a specific exception.
#[pyfunction]
#[pyo3(signature = (expected_exception, *, r#match = None))]
pub fn raises(
    py: Python<'_>,
    expected_exception: Py<PyAny>,
    r#match: Option<String>,
) -> PyResult<RaisesContext> {
    let exc_info = Py::new(py, ExceptionInfo::new())?;
    Ok(RaisesContext {
        expected_exception,
        match_pattern: r#match,
        exc_info,
    })
}
