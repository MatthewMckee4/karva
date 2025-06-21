use pyo3::{
    prelude::*,
    types::{PyDict, PyTuple},
};

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

    pub fn __call__(
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
            function,
            name: func_name,
            scope: self.scope.clone(),
        };

        Ok(fixture_def)
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
    pub const fn new(function: PyObject, name: String, scope: String) -> Self {
        Self {
            name,
            scope,
            function,
        }
    }

    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<PyObject> {
        self.function.call(py, args, kwargs)
    }
}
