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
    #[pyo3(get, set)]
    pub auto_use: bool,
}

#[pymethods]
impl FixtureFunctionMarker {
    #[new]
    #[pyo3(signature = (scope="function".to_string(), name=None, auto_use=false))]
    #[must_use]
    pub const fn new(scope: String, name: Option<String>, auto_use: bool) -> Self {
        Self {
            scope,
            name,
            auto_use,
        }
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
            auto_use: self.auto_use,
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
    #[pyo3(get, set)]
    pub auto_use: bool,
    pub function: PyObject,
}

#[pymethods]
impl FixtureFunctionDefinition {
    #[new]
    #[must_use]
    pub const fn new(function: PyObject, name: String, scope: String, auto_use: bool) -> Self {
        Self {
            name,
            scope,
            auto_use,
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

#[pyfunction(name = "fixture")]
#[pyo3(signature = (func=None, *, scope="function", name=None, auto_use=false))]
pub fn fixture_decorator(
    py: Python<'_>,
    func: Option<PyObject>,
    scope: &str,
    name: Option<&str>,
    auto_use: bool,
) -> PyResult<PyObject> {
    let marker = FixtureFunctionMarker::new(scope.to_string(), name.map(String::from), auto_use);
    if let Some(f) = func {
        let fixture_def = marker.__call__(py, f)?;
        Ok(Py::new(py, fixture_def)?.into_any())
    } else {
        Ok(Py::new(py, marker)?.into_any())
    }
}
