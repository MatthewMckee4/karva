use pyo3::IntoPyObjectExt;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};

use crate::extensions::tags::python::PyTags;

/// Represents a test node that can be accessed via request.node
///
/// This provides access to the test's tags/markers similar to pytest's Node.
#[pyclass]
#[derive(Debug, Clone)]
pub struct TestNode {
    /// Name of the test (e.g., `"test_foo"` or `"test_foo[param1]"`)
    /// Exposed as read-only property for pytest compatibility
    #[pyo3(get)]
    pub name: String,
    tags: PyTags,
    /// Original pytest marks for compatibility
    pytest_marks: Option<Py<PyAny>>,
}

#[pymethods]
impl TestNode {
    /// Get the first (closest) tag/marker with the given name.
    ///
    /// This is similar to pytest's `get_closest_marker`.
    /// It checks both Karva tags and pytest markers for compatibility.
    /// Returns None if no tag/marker with the given name is found.
    pub fn get_closest_tag(&self, py: Python<'_>, name: &str) -> Option<Py<PyAny>> {
        // First check pytest marks if available (to preserve original mark objects)
        if let Some(ref pytest_marks) = self.pytest_marks {
            if let Ok(marks_list) = pytest_marks.extract::<Vec<Bound<'_, PyAny>>>(py) {
                for mark in marks_list {
                    if let Ok(mark_name) = mark.getattr("name") {
                        if let Ok(mark_name_str) = mark_name.extract::<String>() {
                            if mark_name_str == name {
                                return Some(mark.unbind());
                            }
                        }
                    }
                }
            }
        }

        // Then check Karva tags
        for tag in &self.tags.inner {
            if tag.name() == name {
                // Convert the tag to a Python object and return it
                if let Ok(py_tag) = tag.clone().into_py_any(py) {
                    return Some(py_tag);
                }
            }
        }

        None
    }

    /// Alias for `get_closest_tag` for pytest compatibility
    pub fn get_closest_marker(&self, py: Python<'_>, name: &str) -> Option<Py<PyAny>> {
        self.get_closest_tag(py, name)
    }
}

impl TestNode {
    pub(crate) fn new(
        test_name: Option<String>,
        tags: PyTags,
        pytest_marks: Option<Py<PyAny>>,
    ) -> Self {
        Self {
            name: test_name.unwrap_or_default(),
            tags,
            pytest_marks,
        }
    }
}

/// Request context object that fixtures can access via the 'request' parameter.
///
/// This provides access to metadata about the current test/fixture context,
/// most notably the current parameter value for parametrized fixtures.
#[pyclass]
#[derive(Debug, Clone)]
pub struct FixtureRequest {
    #[pyo3(get)]
    pub param: Option<Py<PyAny>>,

    #[pyo3(get)]
    pub node: Option<Py<TestNode>>,
}

impl FixtureRequest {
    pub(crate) fn new(
        py: Python<'_>,
        param: Option<Py<PyAny>>,
        node: Option<TestNode>,
    ) -> PyResult<Self> {
        let node_py = node.map(|n| Py::new(py, n)).transpose()?;
        Ok(Self {
            param,
            node: node_py,
        })
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
