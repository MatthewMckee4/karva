use pyo3::{
    prelude::*,
    types::{PyDict, PyTuple},
};

/// Request context object that fixtures can access via the 'request' parameter.
///
/// This provides access to metadata about the current test/fixture context,
/// most notably the current parameter value for parametrized fixtures.
#[pyclass]
#[derive(Debug, Clone)]
pub struct FixtureRequest {
    #[pyo3(get)]
    pub param: Option<Py<PyAny>>,
}

impl FixtureRequest {
    pub(crate) const fn new(param: Option<Py<PyAny>>) -> Self {
        Self { param }
    }
}

#[pyclass]
pub struct FixtureFunctionMarker {
    #[pyo3(get)]
    pub scope: Py<PyAny>,

    #[pyo3(get)]
    pub name: Option<String>,

    #[pyo3(get)]
    pub auto_use: bool,

    #[pyo3(get)]
    pub params: Option<Vec<Py<PyAny>>>,
}

impl FixtureFunctionMarker {
    pub fn new(
        py: Python<'_>,
        scope: Option<Py<PyAny>>,
        name: Option<String>,
        auto_use: bool,
        params: Option<Vec<Py<PyAny>>>,
    ) -> Self {
        let scope =
            scope.unwrap_or_else(|| "function".to_string().into_pyobject(py).unwrap().into());

        Self {
            scope,
            name,
            auto_use,
            params,
        }
    }
}

#[pymethods]
impl FixtureFunctionMarker {
    pub fn __call__(
        &self,
        py: Python<'_>,
        function: Py<PyAny>,
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
            params: self.params.clone(),
        };

        Ok(fixture_def)
    }
}

#[derive(Debug)]
#[pyclass]
pub struct FixtureFunctionDefinition {
    #[pyo3(get)]
    pub name: String,

    #[pyo3(get)]
    pub scope: Py<PyAny>,

    #[pyo3(get)]
    pub auto_use: bool,

    #[pyo3(get)]
    pub params: Option<Vec<Py<PyAny>>>,

    #[pyo3(get)]
    pub function: Py<PyAny>,
}

#[pymethods]
impl FixtureFunctionDefinition {
    #[pyo3(signature = (*args, **kwargs))]
    fn __call__(
        &self,
        py: Python<'_>,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        self.function.call(py, args, kwargs)
    }
}

#[pyfunction(name = "fixture")]
#[pyo3(signature = (func=None, *, scope=None, name=None, auto_use=false, params=None))]
pub fn fixture_decorator(
    py: Python<'_>,
    func: Option<Py<PyAny>>,
    scope: Option<Py<PyAny>>,
    name: Option<&str>,
    auto_use: bool,
    params: Option<Vec<Py<PyAny>>>,
) -> PyResult<Py<PyAny>> {
    let marker = FixtureFunctionMarker::new(py, scope, name.map(String::from), auto_use, params);
    if let Some(f) = func {
        let fixture_def = marker.__call__(py, f)?;
        Ok(Py::new(py, fixture_def)?.into_any())
    } else {
        Ok(Py::new(py, marker)?.into_any())
    }
}

// InvalidFixtureError exception that can be raised when a fixture is invalid
pyo3::create_exception!(karva, InvalidFixtureError, pyo3::exceptions::PyException);
