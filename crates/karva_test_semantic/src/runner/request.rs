//! Rust-backed implementation of pytest's public fixture request contract.

use std::cell::RefCell;
use std::rc::Rc;

use pyo3::exceptions::{PyAttributeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyType};

use crate::extensions::fixtures::FixtureScope;

pyo3::create_exception!(karva, FixtureLookupError, pyo3::exceptions::PyLookupError);

type GetFixture = dyn Fn(Python<'_>, &str) -> Result<Py<PyAny>, RequestFixtureError>;
type AddFinalizer = dyn Fn(Python<'_>, Py<PyAny>) -> PyResult<()>;
type RequestMarkers = Rc<RefCell<Vec<Py<PyAny>>>>;

/// Failure returned by runtime fixture lookup before Python exception conversion.
pub(super) enum RequestFixtureError {
    Lookup(String),
    Python(PyErr),
}

/// Runtime operations captured by one request object without borrowing the package runner.
#[derive(Clone)]
pub(super) struct RequestRuntime {
    get_fixture: Rc<GetFixture>,
    add_finalizer: Rc<AddFinalizer>,
}

impl RequestRuntime {
    pub(super) fn new(
        get_fixture: impl Fn(Python<'_>, &str) -> Result<Py<PyAny>, RequestFixtureError> + 'static,
        add_finalizer: impl Fn(Python<'_>, Py<PyAny>) -> PyResult<()> + 'static,
    ) -> Self {
        Self {
            get_fixture: Rc::new(get_fixture),
            add_finalizer: Rc::new(add_finalizer),
        }
    }
}

/// Shared test metadata used by top-level and fixture-level requests.
pub(super) struct RequestContext {
    function: Py<PyAny>,
    instance: Py<PyAny>,
    module: Py<PyAny>,
    path: Py<PyAny>,
    config: Py<RequestConfig>,
    session: Py<RequestSession>,
    keywords: Py<PyDict>,
    fixture_names: RefCell<Vec<String>>,
    markers: RequestMarkers,
    test_name: String,
    module_name: String,
    node_id: String,
}

impl RequestContext {
    pub(super) fn new(
        py: Python<'_>,
        function: Py<PyAny>,
        module_name: &str,
        path: &str,
        root_path: &str,
        test_name: String,
        fixture_names: Vec<String>,
    ) -> PyResult<Self> {
        let module = py.import(module_name)?.into_any().unbind();
        let path_object = python_path(py, path)?;
        let root_path_object = python_path(py, root_path)?;
        let (markers, keywords) = request_markers(py, &function)?;
        let config = Py::new(
            py,
            RequestConfig {
                rootpath: root_path_object,
                inipath: None,
                options: PyDict::new(py).unbind(),
                ini: PyDict::new(py).unbind(),
            },
        )?;
        let session = Py::new(
            py,
            RequestSession {
                config: config.clone_ref(py),
                testscollected: 0,
                items: PyList::empty(py).unbind(),
            },
        )?;
        let node_id = format!("{path}::{test_name}");

        Ok(Self {
            function,
            instance: py.None(),
            module,
            path: path_object,
            config,
            session,
            keywords,
            fixture_names: RefCell::new(fixture_names),
            markers,
            test_name,
            module_name: module_name.to_string(),
            node_id,
        })
    }

    pub(super) fn add_fixture_name(&self, name: &str) {
        let mut fixture_names = self.fixture_names.borrow_mut();
        if !fixture_names.iter().any(|existing| existing == name) {
            fixture_names.push(name.to_string());
        }
    }

    fn node(&self, py: Python<'_>, scope: FixtureScope) -> PyResult<Py<RequestNode>> {
        let (name, node_id, path) = match scope {
            FixtureScope::Function => (
                self.test_name.clone(),
                self.node_id.clone(),
                self.path.clone_ref(py),
            ),
            FixtureScope::Module => (
                self.module_name.clone(),
                self.path.bind(py).to_string(),
                self.path.clone_ref(py),
            ),
            FixtureScope::Package => {
                let path = self.path.bind(py).to_string();
                let package_path = std::path::Path::new(&path)
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new(&path));
                let name = package_path
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or_default()
                    .to_string();
                let node_id = package_path.to_string_lossy().into_owned();
                (name, node_id.clone(), python_path(py, &node_id)?)
            }
            FixtureScope::Session => (
                String::new(),
                String::new(),
                self.config.borrow(py).rootpath.clone_ref(py),
            ),
        };
        Py::new(
            py,
            RequestNode {
                name,
                node_id,
                path,
                config: self.config.clone_ref(py),
                session: self.session.clone_ref(py),
                keywords: self.keywords.clone_ref(py),
                markers: Rc::clone(&self.markers),
            },
        )
    }
}

fn python_path(py: Python<'_>, path: &str) -> PyResult<Py<PyAny>> {
    py.import("pathlib")?
        .getattr("Path")?
        .call1((path,))
        .map(Bound::unbind)
}

fn request_markers(py: Python<'_>, function: &Py<PyAny>) -> PyResult<(RequestMarkers, Py<PyDict>)> {
    let keywords = PyDict::new(py).unbind();
    let markers = match function.getattr(py, "pytestmark") {
        Ok(markers) => markers
            .bind(py)
            .try_iter()?
            .map(|marker| marker.and_then(|marker| normalize_marker(py, &marker)))
            .collect::<PyResult<Vec<_>>>()?,
        Err(error) if error.is_instance_of::<PyAttributeError>(py) => Vec::new(),
        Err(error) => return Err(error),
    };
    for marker in &markers {
        keywords
            .bind(py)
            .set_item(marker_name(py, marker)?, marker)?;
    }
    Ok((Rc::new(RefCell::new(markers)), keywords))
}

/// Karva run configuration exposed through `request.config`.
#[pyclass(name = "Config", module = "karva", frozen)]
pub struct RequestConfig {
    rootpath: Py<PyAny>,
    inipath: Option<Py<PyAny>>,
    options: Py<PyDict>,
    ini: Py<PyDict>,
}

#[pymethods]
impl RequestConfig {
    #[getter]
    fn rootpath(&self, py: Python<'_>) -> Py<PyAny> {
        self.rootpath.clone_ref(py)
    }

    #[getter]
    fn inipath(&self, py: Python<'_>) -> Option<Py<PyAny>> {
        self.inipath.as_ref().map(|path| path.clone_ref(py))
    }

    #[pyo3(signature = (name, default=None))]
    fn getoption(
        &self,
        py: Python<'_>,
        name: &str,
        default: Option<Py<PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let normalized_name = name.trim_start_matches('-').replace('-', "_");
        if let Some(value) = self.options.bind(py).get_item(&normalized_name)? {
            return Ok(value.unbind());
        }
        default.map_or_else(
            || Err(PyValueError::new_err(format!("no option named {name}"))),
            |value| Ok(value.clone_ref(py)),
        )
    }

    fn getini(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        if let Some(value) = self.ini.bind(py).get_item(name)? {
            return Ok(value.unbind());
        }
        match name {
            "markers" | "testpaths" => Ok(PyList::empty(py).into_any().unbind()),
            _ => Err(PyValueError::new_err(format!(
                "unknown configuration value: {name}"
            ))),
        }
    }
}

/// Karva worker session exposed through `request.session`.
#[pyclass(name = "Session", module = "karva", frozen)]
pub struct RequestSession {
    config: Py<RequestConfig>,
    testscollected: usize,
    items: Py<PyList>,
}

#[pymethods]
impl RequestSession {
    #[getter]
    fn config(&self, py: Python<'_>) -> Py<RequestConfig> {
        self.config.clone_ref(py)
    }

    #[getter]
    fn testscollected(&self) -> usize {
        self.testscollected
    }

    #[getter]
    fn items(&self, py: Python<'_>) -> Py<PyAny> {
        self.items.clone_ref(py).into_any()
    }
}

/// Active collection node exposed through `request.node`.
#[pyclass(name = "Node", module = "karva", unsendable)]
pub struct RequestNode {
    name: String,
    node_id: String,
    path: Py<PyAny>,
    config: Py<RequestConfig>,
    session: Py<RequestSession>,
    keywords: Py<PyDict>,
    markers: RequestMarkers,
}

impl RequestNode {
    fn add_marker_value(
        &self,
        py: Python<'_>,
        marker: &Bound<'_, PyAny>,
        append: bool,
    ) -> PyResult<()> {
        let marker = normalize_marker(py, marker)?;
        let name = marker_name(py, &marker)?;
        self.keywords.bind(py).set_item(&name, &marker)?;
        if append {
            self.markers.borrow_mut().push(marker);
        } else {
            self.markers.borrow_mut().insert(0, marker);
        }
        Ok(())
    }
}

#[pymethods]
impl RequestNode {
    #[getter]
    fn name(&self) -> &str {
        &self.name
    }

    #[getter]
    fn nodeid(&self) -> &str {
        &self.node_id
    }

    #[getter]
    fn originalname(&self) -> &str {
        &self.name
    }

    #[getter]
    fn path(&self, py: Python<'_>) -> Py<PyAny> {
        self.path.clone_ref(py)
    }

    #[getter]
    fn config(&self, py: Python<'_>) -> Py<RequestConfig> {
        self.config.clone_ref(py)
    }

    #[getter]
    fn session(&self, py: Python<'_>) -> Py<RequestSession> {
        self.session.clone_ref(py)
    }

    #[getter]
    fn keywords(&self, py: Python<'_>) -> Py<PyDict> {
        self.keywords.clone_ref(py)
    }

    #[pyo3(signature = (marker, append=true))]
    fn add_marker(&self, py: Python<'_>, marker: &Bound<'_, PyAny>, append: bool) -> PyResult<()> {
        self.add_marker_value(py, marker, append)
    }

    #[pyo3(signature = (name=None))]
    fn iter_markers(&self, py: Python<'_>, name: Option<&str>) -> PyResult<Py<PyAny>> {
        let markers = self
            .markers
            .borrow()
            .iter()
            .filter(|marker| {
                name.is_none_or(|name| marker_name(py, marker).is_ok_and(|found| found == name))
            })
            .map(|marker| marker.clone_ref(py))
            .collect::<Vec<_>>();
        Ok(PyList::new(py, markers)?.into_any().unbind())
    }

    #[pyo3(signature = (name, default=None))]
    fn get_closest_marker(
        &self,
        py: Python<'_>,
        name: &str,
        default: Option<Py<PyAny>>,
    ) -> Py<PyAny> {
        self.markers
            .borrow()
            .iter()
            .rev()
            .find(|marker| marker_name(py, marker).is_ok_and(|found| found == name))
            .map_or_else(
                || default.map_or_else(|| py.None(), |value| value.clone_ref(py)),
                |marker| marker.clone_ref(py),
            )
    }

    fn __repr__(&self) -> String {
        format!("<Node {}>", self.node_id)
    }
}

fn normalize_marker(py: Python<'_>, marker: &Bound<'_, PyAny>) -> PyResult<Py<PyAny>> {
    if let Ok(name) = marker.extract::<String>() {
        return py
            .import("pytest")?
            .getattr("mark")?
            .getattr(name)?
            .getattr("mark")
            .map(Bound::unbind);
    }
    marker
        .getattr("mark")
        .map(Bound::unbind)
        .or_else(|_| Ok(marker.clone().unbind()))
}

fn marker_name(py: Python<'_>, marker: &Py<PyAny>) -> PyResult<String> {
    marker.getattr(py, "name")?.extract(py)
}

/// Pytest-compatible request object backed by Karva's Rust fixture runtime.
#[pyclass(name = "FixtureRequest", module = "karva", unsendable)]
pub struct FixtureRequest {
    context: Rc<RequestContext>,
    runtime: RequestRuntime,
    fixture_name: Option<String>,
    scope: FixtureScope,
    param: Option<Py<PyAny>>,
    param_index: Option<usize>,
    node: Py<RequestNode>,
    fixture_stack: Py<PyList>,
}

impl FixtureRequest {
    pub(super) fn new(
        py: Python<'_>,
        context: Rc<RequestContext>,
        runtime: RequestRuntime,
        fixture_name: Option<String>,
        scope: FixtureScope,
        param: Option<Py<PyAny>>,
        param_index: Option<usize>,
    ) -> PyResult<Self> {
        let node = context.node(py, scope)?;
        Ok(Self {
            context,
            runtime,
            fixture_name,
            scope,
            param,
            param_index,
            node,
            fixture_stack: PyList::empty(py).unbind(),
        })
    }
}

#[pymethods]
impl FixtureRequest {
    #[getter]
    fn fixturename(&self) -> Option<&str> {
        self.fixture_name.as_deref()
    }

    #[getter]
    fn param(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.param
            .as_ref()
            .map(|param| param.clone_ref(py))
            .ok_or_else(|| PyAttributeError::new_err("param"))
    }

    #[getter]
    fn param_index(&self) -> PyResult<usize> {
        self.param_index
            .ok_or_else(|| PyAttributeError::new_err("param_index"))
    }

    #[getter]
    fn scope(&self) -> &'static str {
        self.scope.name()
    }

    #[getter]
    fn fixturenames(&self) -> Vec<String> {
        self.context.fixture_names.borrow().clone()
    }

    #[getter]
    fn node(&self, py: Python<'_>) -> Py<RequestNode> {
        self.node.clone_ref(py)
    }

    #[getter]
    fn config(&self, py: Python<'_>) -> Py<RequestConfig> {
        self.context.config.clone_ref(py)
    }

    #[getter]
    fn function(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        if self.scope != FixtureScope::Function {
            return Err(PyAttributeError::new_err(format!(
                "function not available in {}-scoped context",
                self.scope.name()
            )));
        }
        Ok(self.context.function.clone_ref(py))
    }

    #[getter]
    fn cls(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        if !matches!(self.scope, FixtureScope::Function) {
            return Err(PyAttributeError::new_err(format!(
                "cls not available in {}-scoped context",
                self.scope.name()
            )));
        }
        Ok(py.None())
    }

    #[getter]
    fn instance(&self, py: Python<'_>) -> Py<PyAny> {
        self.context.instance.clone_ref(py)
    }

    #[getter]
    fn module(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        if matches!(self.scope, FixtureScope::Package | FixtureScope::Session) {
            return Err(PyAttributeError::new_err(format!(
                "module not available in {}-scoped context",
                self.scope.name()
            )));
        }
        Ok(self.context.module.clone_ref(py))
    }

    #[getter]
    fn path(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        if self.scope == FixtureScope::Session {
            return Err(PyAttributeError::new_err(
                "path not available in session-scoped context",
            ));
        }
        Ok(self.node.borrow(py).path.clone_ref(py))
    }

    #[getter]
    fn keywords(&self, py: Python<'_>) -> Py<PyDict> {
        self.context.keywords.clone_ref(py)
    }

    #[getter]
    fn session(&self, py: Python<'_>) -> Py<RequestSession> {
        self.context.session.clone_ref(py)
    }

    fn addfinalizer(&self, py: Python<'_>, finalizer: Py<PyAny>) -> PyResult<()> {
        if !finalizer.bind(py).is_callable() {
            return Err(PyTypeError::new_err("finalizer must be callable"));
        }
        (self.runtime.add_finalizer)(py, finalizer)
    }

    fn applymarker(&self, py: Python<'_>, marker: &Bound<'_, PyAny>) -> PyResult<()> {
        self.node.borrow(py).add_marker_value(py, marker, true)
    }

    fn raiseerror(slf: Py<Self>, py: Python<'_>, message: Option<String>) -> PyResult<()> {
        Err(fixture_lookup_error(py, None, slf, message))
    }

    fn getfixturevalue(slf: Py<Self>, py: Python<'_>, argname: &str) -> PyResult<Py<PyAny>> {
        if argname == "request" {
            return Ok(slf.into_any());
        }
        let get_fixture = slf.borrow(py).runtime.get_fixture.clone();
        match get_fixture(py, argname) {
            Ok(value) => Ok(value),
            Err(RequestFixtureError::Python(error)) => Err(error),
            Err(RequestFixtureError::Lookup(message)) => {
                Err(fixture_lookup_error(py, Some(argname), slf, Some(message)))
            }
        }
    }

    fn _get_fixturestack(&self, py: Python<'_>) -> Py<PyAny> {
        self.fixture_stack.clone_ref(py).into_any()
    }

    fn __repr__(&self) -> String {
        self.fixture_name.as_ref().map_or_else(
            || format!("<FixtureRequest for {}>", self.context.node_id),
            |name| format!("<SubRequest {name:?} for {}>", self.context.node_id),
        )
    }
}

fn fixture_lookup_error(
    py: Python<'_>,
    argname: Option<&str>,
    request: Py<FixtureRequest>,
    message: Option<String>,
) -> PyErr {
    let argname = argname.map(str::to_string);
    if let Ok(exception_type) = py
        .import("pytest")
        .and_then(|pytest| pytest.getattr("FixtureLookupError"))
        && let Ok(exception_type) = exception_type.cast_into::<PyType>()
    {
        return PyErr::from_type(exception_type, (argname, request, message));
    }
    FixtureLookupError::new_err(message.unwrap_or_else(|| {
        argname.as_deref().map_or_else(
            || "fixture lookup failed".to_string(),
            |argname| format!("fixture {argname:?} not found"),
        )
    }))
}
