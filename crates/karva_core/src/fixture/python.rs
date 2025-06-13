use pyo3::prelude::*;

#[pyclass]
pub struct FixtureFunctionMarker {
    #[pyo3(get, set)]
    pub scope: String,
    #[pyo3(get, set)]
    pub name: Option<String>,
}

#[pymethods]
impl FixtureFunctionMarker {
    #[new]
    #[pyo3(signature = (scope="function".to_string(), name=None))]
    #[must_use]
    pub const fn new(scope: String, name: Option<String>) -> Self {
        Self { scope, name }
    }

    pub fn call_with_function(
        &self,
        py: Python<'_>,
        function: PyObject,
    ) -> PyResult<FixtureFunctionDefinition> {
        let func_name = if let Some(ref name) = self.name {
            name.clone()
        } else {
            function.getattr(py, "__name__")?.extract::<String>(py)?
        };

        let fixture_def = FixtureFunctionDefinition {
            name: func_name,
            scope: self.scope.clone(),
            function,
        };

        Ok(fixture_def)
    }

    fn __call__(&self, py: Python<'_>, function: PyObject) -> PyResult<FixtureFunctionDefinition> {
        self.call_with_function(py, function)
    }
}

#[derive(Debug)]
#[pyclass]
pub struct FixtureFunctionDefinition {
    #[pyo3(get, set)]
    pub name: String,
    #[pyo3(get, set)]
    pub scope: String,
    pub function: PyObject,
}

#[pymethods]
impl FixtureFunctionDefinition {
    #[new]
    #[must_use]
    pub const fn new(name: String, scope: String, function: PyObject) -> Self {
        Self {
            name,
            scope,
            function,
        }
    }

    fn __call__(&self, py: Python<'_>) -> PyResult<PyObject> {
        self.function.call0(py)
    }
}
