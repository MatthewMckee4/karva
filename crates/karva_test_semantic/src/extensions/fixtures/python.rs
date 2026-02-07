use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

/// Marker object created when `@fixture(...)` is called with arguments.
///
/// This is an intermediate object that captures the decorator arguments
/// and waits to be called with the actual function to decorate.
#[pyclass]
pub struct FixtureFunctionMarker {
    /// The scope at which the fixture value should be cached.
    #[pyo3(get)]
    pub scope: Py<PyAny>,

    /// Optional custom name for the fixture (defaults to function name).
    #[pyo3(get)]
    pub name: Option<String>,

    /// Whether this fixture should be automatically used.
    #[pyo3(get)]
    pub auto_use: bool,
}

impl FixtureFunctionMarker {
    pub fn new(
        py: Python<'_>,
        scope: Option<Py<PyAny>>,
        name: Option<String>,
        auto_use: bool,
    ) -> Self {
        let scope =
            scope.unwrap_or_else(|| "function".to_string().into_pyobject(py).unwrap().into());

        Self {
            scope,
            name,
            auto_use,
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
            scope: self.scope.clone_ref(py),
            auto_use: self.auto_use,
        };

        Ok(fixture_def)
    }
}

/// The final decorated fixture function with all metadata attached.
///
/// This object wraps the original Python function and carries the fixture
/// configuration (scope, name, `auto_use`) for later discovery.
#[derive(Debug)]
#[pyclass]
pub struct FixtureFunctionDefinition {
    /// The fixture's name (either custom or derived from function name).
    #[pyo3(get)]
    pub name: String,

    /// The scope at which the fixture value should be cached.
    #[pyo3(get)]
    pub scope: Py<PyAny>,

    /// Whether this fixture should be automatically used.
    #[pyo3(get)]
    pub auto_use: bool,

    /// The underlying Python function that produces the fixture value.
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
#[pyo3(signature = (func=None, *, scope=None, name=None, auto_use=false))]
pub fn fixture_decorator(
    py: Python<'_>,
    func: Option<Py<PyAny>>,
    scope: Option<Py<PyAny>>,
    name: Option<&str>,
    auto_use: bool,
) -> PyResult<Py<PyAny>> {
    let marker = FixtureFunctionMarker::new(py, scope, name.map(String::from), auto_use);
    if let Some(f) = func {
        let fixture_def = marker.__call__(py, f)?;
        Ok(Py::new(py, fixture_def)?.into_any())
    } else {
        Ok(Py::new(py, marker)?.into_any())
    }
}

// InvalidFixtureError exception that can be raised when a fixture is invalid
pyo3::create_exception!(karva, InvalidFixtureError, pyo3::exceptions::PyException);
