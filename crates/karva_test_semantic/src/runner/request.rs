//! Rust-backed implementation of pytest's public fixture request contract.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pyo3::exceptions::{PyAttributeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple, PyType};

use crate::extensions::fixtures::FixtureScope;
use crate::extensions::tags::{RuntimeTags, Tags};

pyo3::create_exception!(karva, FixtureLookupError, pyo3::exceptions::PyLookupError);

type GetFixture = dyn Fn(Python<'_>, &str) -> Result<Py<PyAny>, RequestFixtureError>;
type AddFinalizer = dyn Fn(Python<'_>, Py<PyAny>) -> PyResult<()>;
type RequestMarkers = Py<PyList>;

/// Lazily allocated pytest-style collection state shared by every request.
pub(super) struct RequestState {
    root_path: String,
    config: Py<RequestConfig>,
    session: Py<RequestSession>,
    session_node: Py<RequestNode>,
    package_nodes: HashMap<String, Py<RequestNode>>,
    module_nodes: HashMap<String, Py<RequestNode>>,
    item_nodes: HashMap<String, Py<RequestNode>>,
}

impl RequestState {
    pub(super) fn new(
        py: Python<'_>,
        root_path: &str,
        verbose: bool,
        max_fail: u32,
        test_function_prefix: &str,
        include_paths: &[String],
    ) -> PyResult<Self> {
        register_with_pytest(py)?;
        let root_path_object = python_path(py, root_path)?;
        let options = PyDict::new(py);
        options.set_item("verbose", u8::from(verbose))?;
        options.set_item("maxfail", max_fail)?;
        options.set_item("exitfirst", max_fail == 1)?;
        let ini = PyDict::new(py);
        ini.set_item("python_functions", [test_function_prefix])?;
        ini.set_item("testpaths", include_paths)?;
        ini.set_item("markers", PyList::empty(py))?;
        ini.set_item("addopts", PyList::empty(py))?;
        ini.set_item("xfail_strict", false)?;
        let config = Py::new(
            py,
            RequestConfig {
                rootpath: root_path_object.clone_ref(py),
                inipath: None,
                options: options.unbind(),
                ini: ini.unbind(),
            },
        )?;
        let session = Py::new(
            py,
            RequestSession {
                config: config.clone_ref(py),
                items: PyList::empty(py).unbind(),
            },
        )?;
        let session_node = Py::new(
            py,
            RequestNode {
                name: String::new(),
                original_name: String::new(),
                node_id: String::new(),
                path: root_path_object,
                config: config.clone_ref(py),
                session: session.clone_ref(py),
                keywords: PyDict::new(py).unbind(),
                descendant_keywords: RefCell::new(Vec::new()),
                markers: PyList::empty(py).unbind(),
                applied_tags: Rc::default(),
                globals: None,
                parent: None,
            },
        )?;
        Ok(Self {
            root_path: root_path.to_string(),
            config,
            session,
            session_node,
            package_nodes: HashMap::new(),
            module_nodes: HashMap::new(),
            item_nodes: HashMap::new(),
        })
    }

    pub(super) fn add_item(
        &mut self,
        py: Python<'_>,
        test: &crate::discovery::DiscoveredTestFunction,
        parameter_id: Option<&str>,
    ) -> PyResult<()> {
        let path = test.name().module_path().path().as_str();
        let package_path = std::path::Path::new(path)
            .parent()
            .unwrap_or_else(|| std::path::Path::new(path))
            .to_string_lossy()
            .into_owned();
        if !self.package_nodes.contains_key(&package_path) {
            let absolute_path = self.absolute_path(&package_path);
            let name = std::path::Path::new(&absolute_path)
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_string();
            let node = request_node(
                py,
                name.clone(),
                name,
                package_path.clone(),
                python_path(py, &absolute_path)?,
                self.config.clone_ref(py),
                self.session.clone_ref(py),
                Vec::new(),
                None,
                Some(self.session_node.clone_ref(py)),
            )?;
            self.package_nodes.insert(package_path.clone(), node);
        }

        if !self.module_nodes.contains_key(path) {
            let module = py.import(test.name().module_path().module_name())?;
            let globals = module.dict().unbind();
            let markers = object_markers(py, module.as_any())?;
            let name = std::path::Path::new(path)
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .unwrap_or_default()
                .to_string();
            let parent = self
                .package_nodes
                .get(&package_path)
                .map(|node| node.clone_ref(py));
            let node = request_node(
                py,
                name.clone(),
                name,
                path.to_string(),
                python_path(py, &self.absolute_path(path))?,
                self.config.clone_ref(py),
                self.session.clone_ref(py),
                markers,
                Some(globals),
                parent,
            )?;
            self.module_nodes.insert(path.to_string(), node);
        }

        let test_node_name = parameter_id.map_or_else(
            || test.name().function_name().to_string(),
            |parameter_id| format!("{}[{parameter_id}]", test.name().function_name()),
        );
        let node_id = format!("{path}::{test_node_name}");
        if self.item_nodes.contains_key(&node_id) {
            return Ok(());
        }
        let globals = test
            .py_function
            .getattr(py, "__globals__")?
            .cast_bound::<PyDict>(py)?
            .clone()
            .unbind();
        let markers = object_markers(py, test.py_function.bind(py))?;
        let parent = self.module_nodes.get(path).map(|node| node.clone_ref(py));
        let node = request_node(
            py,
            test_node_name,
            test.name().function_name().to_string(),
            node_id.clone(),
            python_path(py, &self.absolute_path(path))?,
            self.config.clone_ref(py),
            self.session.clone_ref(py),
            markers,
            Some(globals),
            parent,
        )?;
        self.session.borrow(py).items.bind(py).append(&node)?;
        self.item_nodes.insert(node_id, node);
        Ok(())
    }

    fn absolute_path(&self, path: &str) -> String {
        let path = std::path::Path::new(path);
        if path.is_absolute() {
            path.to_string_lossy().into_owned()
        } else {
            std::path::Path::new(&self.root_path)
                .join(path)
                .to_string_lossy()
                .into_owned()
        }
    }

    fn nodes(
        &self,
        py: Python<'_>,
        path: &str,
        test_name: &str,
        parameter_id: Option<&str>,
    ) -> PyResult<RequestNodes> {
        let test_node_name = parameter_id.map_or_else(
            || test_name.to_string(),
            |parameter_id| format!("{test_name}[{parameter_id}]"),
        );
        let node_id = format!("{path}::{test_node_name}");
        let package_path = std::path::Path::new(path)
            .parent()
            .unwrap_or_else(|| std::path::Path::new(path))
            .to_string_lossy();
        Ok(RequestNodes {
            function: self
                .item_nodes
                .get(&node_id)
                .ok_or_else(|| PyValueError::new_err(format!("request item {node_id:?} missing")))?
                .clone_ref(py),
            module: self
                .module_nodes
                .get(path)
                .ok_or_else(|| PyValueError::new_err(format!("request module {path:?} missing")))?
                .clone_ref(py),
            package: self
                .package_nodes
                .get(package_path.as_ref())
                .ok_or_else(|| {
                    PyValueError::new_err(format!("request package {package_path:?} missing"))
                })?
                .clone_ref(py),
            session: self.session_node.clone_ref(py),
        })
    }

    pub(super) fn reorder_items(
        &self,
        py: Python<'_>,
        node_ids: impl IntoIterator<Item = String>,
    ) -> PyResult<()> {
        let session = self.session.borrow(py);
        let items = session.items.bind(py);
        items.call_method0("clear")?;
        for node_id in node_ids {
            if let Some(node) = self.item_nodes.get(&node_id) {
                items.append(node)?;
            }
        }
        Ok(())
    }
}

struct RequestNodes {
    function: Py<RequestNode>,
    module: Py<RequestNode>,
    package: Py<RequestNode>,
    session: Py<RequestNode>,
}

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
    config: Py<RequestConfig>,
    session: Py<RequestSession>,
    fixture_names: RefCell<Vec<String>>,
    nodes: RequestNodes,
    node_id: String,
}

pub(super) struct RequestMetadata<'a> {
    pub(super) module_name: &'a str,
    pub(super) path: &'a str,
    pub(super) test_name: &'a str,
    pub(super) parameter_id: Option<&'a str>,
    pub(super) fixture_names: Vec<String>,
}

impl RequestContext {
    pub(super) fn new(
        py: Python<'_>,
        state: &RequestState,
        function: Py<PyAny>,
        metadata: RequestMetadata<'_>,
    ) -> PyResult<Self> {
        let RequestMetadata {
            module_name,
            path,
            test_name,
            parameter_id,
            fixture_names,
        } = metadata;
        let module = py.import(module_name)?.into_any().unbind();
        let test_node_name = parameter_id.map_or_else(
            || test_name.to_string(),
            |parameter_id| format!("{test_name}[{parameter_id}]"),
        );
        let node_id = format!("{path}::{test_node_name}");
        let nodes = state.nodes(py, path, test_name, parameter_id)?;

        Ok(Self {
            function,
            instance: py.None(),
            module,
            config: state.config.clone_ref(py),
            session: state.session.clone_ref(py),
            fixture_names: RefCell::new(fixture_names),
            nodes,
            node_id,
        })
    }

    pub(super) fn add_fixture_name(&self, name: &str) {
        let mut fixture_names = self.fixture_names.borrow_mut();
        if !fixture_names.iter().any(|existing| existing == name) {
            fixture_names.push(name.to_string());
        }
    }

    pub(super) fn applied_tags(&self, py: Python<'_>) -> RuntimeTags {
        self.nodes.function.borrow(py).all_applied_tags(py)
    }

    fn node(&self, py: Python<'_>, scope: FixtureScope) -> Py<RequestNode> {
        match scope {
            FixtureScope::Function => self.nodes.function.clone_ref(py),
            FixtureScope::Module => self.nodes.module.clone_ref(py),
            FixtureScope::Package => self.nodes.package.clone_ref(py),
            FixtureScope::Session => self.nodes.session.clone_ref(py),
        }
    }
}

fn python_path(py: Python<'_>, path: &str) -> PyResult<Py<PyAny>> {
    py.import("pathlib")?
        .getattr("Path")?
        .call1((path,))
        .map(Bound::unbind)
}

fn object_markers(py: Python<'_>, object: &Bound<'_, PyAny>) -> PyResult<Vec<Py<PyAny>>> {
    match object.getattr("pytestmark") {
        Ok(markers) => {
            if let Ok(markers) = markers.try_iter() {
                markers
                    .map(|marker| marker.and_then(|marker| normalize_marker(py, &marker)))
                    .collect()
            } else {
                Ok(vec![normalize_marker(py, &markers)?])
            }
        }
        Err(error) if error.is_instance_of::<PyAttributeError>(py) => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

#[expect(clippy::too_many_arguments)]
fn request_node(
    py: Python<'_>,
    name: String,
    original_name: String,
    node_id: String,
    path: Py<PyAny>,
    config: Py<RequestConfig>,
    session: Py<RequestSession>,
    markers: Vec<Py<PyAny>>,
    globals: Option<Py<PyDict>>,
    parent: Option<Py<RequestNode>>,
) -> PyResult<Py<RequestNode>> {
    let keywords = PyDict::new(py).unbind();
    if let Some(parent) = &parent {
        for (key, value) in parent.borrow(py).keywords.bind(py).iter() {
            keywords.bind(py).set_item(key, value)?;
        }
    }
    if !name.is_empty() {
        keywords.bind(py).set_item(&name, true)?;
    }
    for marker in &markers {
        keywords
            .bind(py)
            .set_item(marker_name(py, marker)?, marker)?;
    }
    if let Some(parent) = &parent {
        parent
            .borrow(py)
            .register_descendant_keywords(py, keywords.clone_ref(py));
    }
    Py::new(
        py,
        RequestNode {
            name,
            original_name,
            node_id,
            path,
            config,
            session,
            keywords,
            descendant_keywords: RefCell::new(Vec::new()),
            markers: PyList::new(py, markers)?.unbind(),
            applied_tags: Rc::default(),
            globals,
            parent,
        },
    )
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

    #[pyo3(signature = (name, *args, **kwargs))]
    fn getoption(
        &self,
        py: Python<'_>,
        name: &str,
        args: &Bound<'_, PyTuple>,
        kwargs: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Py<PyAny>> {
        if args.len() > 2 {
            return Err(PyTypeError::new_err(format!(
                "getoption() takes at most 3 arguments ({} given)",
                args.len() + 1
            )));
        }
        let mut default = args.get_item(0).ok().map(Bound::unbind);
        let mut skip = args
            .get_item(1)
            .ok()
            .map(|skip| skip.extract::<bool>())
            .transpose()?
            .unwrap_or(false);
        if let Some(kwargs) = kwargs {
            for (key, value) in kwargs.iter() {
                let key = key.extract::<String>()?;
                match key.as_str() {
                    "default" if default.is_none() => default = Some(value.unbind()),
                    "skip" if args.len() < 2 => skip = value.extract::<bool>()?,
                    "default" | "skip" => {
                        return Err(PyTypeError::new_err(format!(
                            "getoption() got multiple values for argument {key:?}"
                        )));
                    }
                    _ => {
                        return Err(PyTypeError::new_err(format!(
                            "getoption() got an unexpected keyword argument {key:?}"
                        )));
                    }
                }
            }
        }
        let normalized_name = match name {
            "-v" | "--verbose" => "verbose".to_string(),
            "-x" | "--exitfirst" => "exitfirst".to_string(),
            _ => name.trim_start_matches('-').replace('-', "_"),
        };
        if let Some(value) = self.options.bind(py).get_item(&normalized_name)? {
            if skip && value.is_none() {
                return py
                    .import("pytest")?
                    .getattr("skip")?
                    .call1((format!("no '{normalized_name}' option found"),))
                    .map(Bound::unbind);
            }
            return Ok(value.unbind());
        }
        if let Some(default) = default {
            return Ok(default);
        }
        if skip {
            return py
                .import("pytest")?
                .getattr("skip")?
                .call1((format!("no '{normalized_name}' option found"),))
                .map(Bound::unbind);
        }
        Err(PyValueError::new_err(format!(
            "no option named '{normalized_name}'"
        )))
    }

    fn getini(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        if let Some(value) = self.ini.bind(py).get_item(name)? {
            return Ok(value.unbind());
        }
        Err(PyValueError::new_err(format!(
            "unknown configuration value: {name}"
        )))
    }
}

/// Karva worker session exposed through `request.session`.
#[pyclass(name = "Session", module = "karva", frozen)]
pub struct RequestSession {
    config: Py<RequestConfig>,
    items: Py<PyList>,
}

#[pymethods]
impl RequestSession {
    #[getter]
    fn config(&self, py: Python<'_>) -> Py<RequestConfig> {
        self.config.clone_ref(py)
    }

    #[getter]
    fn testscollected(&self, py: Python<'_>) -> usize {
        self.items.bind(py).len()
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
    original_name: String,
    node_id: String,
    path: Py<PyAny>,
    config: Py<RequestConfig>,
    session: Py<RequestSession>,
    keywords: Py<PyDict>,
    descendant_keywords: RefCell<Vec<Py<PyDict>>>,
    markers: RequestMarkers,
    applied_tags: Rc<RefCell<RuntimeTags>>,
    globals: Option<Py<PyDict>>,
    parent: Option<Py<Self>>,
}

impl RequestNode {
    fn register_descendant_keywords(&self, py: Python<'_>, keywords: Py<PyDict>) {
        self.descendant_keywords
            .borrow_mut()
            .push(keywords.clone_ref(py));
        if let Some(parent) = &self.parent {
            parent.borrow(py).register_descendant_keywords(py, keywords);
        }
    }

    fn all_markers(&self, py: Python<'_>) -> Vec<Py<PyAny>> {
        let mut markers = self
            .markers
            .bind(py)
            .iter()
            .map(Bound::unbind)
            .collect::<Vec<_>>();
        if let Some(parent) = &self.parent {
            markers.extend(parent.borrow(py).all_markers(py));
        }
        markers
    }

    fn all_applied_tags(&self, py: Python<'_>) -> RuntimeTags {
        let mut tags = self.applied_tags.borrow().clone();
        if let Some(parent) = &self.parent {
            tags.extend_runtime(&parent.borrow(py).all_applied_tags(py));
        }
        tags
    }

    fn add_marker_value(
        &self,
        py: Python<'_>,
        marker: &Bound<'_, PyAny>,
        append: bool,
    ) -> PyResult<()> {
        let marker = normalize_marker(py, marker)?;
        if let Some(tags) = Tags::from_pytest_marks(
            py,
            &marker,
            self.globals.as_ref().map(|globals| globals.bind(py)),
        )? {
            self.applied_tags.borrow_mut().extend(&tags);
        }
        let name = marker_name(py, &marker)?;
        self.keywords.bind(py).set_item(&name, &marker)?;
        for keywords in &*self.descendant_keywords.borrow() {
            keywords.bind(py).set_item(&name, &marker)?;
        }
        if append {
            self.markers.bind(py).append(marker)?;
        } else {
            self.markers.bind(py).insert(0, marker)?;
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
        &self.original_name
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
    fn parent(&self, py: Python<'_>) -> Option<Py<Self>> {
        self.parent.as_ref().map(|parent| parent.clone_ref(py))
    }

    #[getter]
    fn own_markers(&self, py: Python<'_>) -> Py<PyList> {
        self.markers.clone_ref(py)
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
            .all_markers(py)
            .into_iter()
            .filter(|marker| {
                name.is_none_or(|name| marker_name(py, marker).is_ok_and(|found| found == name))
            })
            .collect::<Vec<_>>();
        Ok(PyList::new(py, markers)?.try_iter()?.into_any().unbind())
    }

    #[pyo3(signature = (name=None))]
    fn iter_markers_with_node(
        slf: Py<Self>,
        py: Python<'_>,
        name: Option<&str>,
    ) -> PyResult<Py<PyAny>> {
        let mut current = Some(slf);
        let mut markers = Vec::new();
        while let Some(node) = current {
            let node_ref = node.borrow(py);
            for marker in node_ref.markers.bind(py).iter() {
                let marker = marker.unbind();
                if name.is_none_or(|name| marker_name(py, &marker).is_ok_and(|found| found == name))
                {
                    markers.push((node.clone_ref(py), marker));
                }
            }
            current = node_ref.parent.as_ref().map(|parent| parent.clone_ref(py));
        }
        Ok(PyList::new(py, markers)?.try_iter()?.into_any().unbind())
    }

    fn iter_parents(slf: Py<Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let mut current = Some(slf);
        let mut parents = Vec::new();
        while let Some(node) = current {
            let parent = node
                .borrow(py)
                .parent
                .as_ref()
                .map(|parent| parent.clone_ref(py));
            parents.push(node);
            current = parent;
        }
        Ok(PyList::new(py, parents)?.try_iter()?.into_any().unbind())
    }

    fn listchain(slf: Py<Self>, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let mut current = Some(slf);
        let mut chain = Vec::new();
        while let Some(node) = current {
            let parent = node
                .borrow(py)
                .parent
                .as_ref()
                .map(|parent| parent.clone_ref(py));
            chain.push(node);
            current = parent;
        }
        chain.reverse();
        Ok(PyList::new(py, chain)?.into_any().unbind())
    }

    fn listnames(slf: Py<Self>, py: Python<'_>) -> Vec<String> {
        let mut current = Some(slf);
        let mut names = Vec::new();
        while let Some(node) = current {
            let node_ref = node.borrow(py);
            names.push(node_ref.name.clone());
            current = node_ref.parent.as_ref().map(|parent| parent.clone_ref(py));
        }
        names.reverse();
        names
    }

    #[pyo3(signature = (name, default=None))]
    fn get_closest_marker(
        &self,
        py: Python<'_>,
        name: &str,
        default: Option<Py<PyAny>>,
    ) -> Py<PyAny> {
        self.all_markers(py)
            .into_iter()
            .find(|marker| marker_name(py, marker).is_ok_and(|found| found == name))
            .unwrap_or_else(|| default.map_or_else(|| py.None(), |value| value.clone_ref(py)))
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
    ) -> Self {
        let node = context.node(py, scope);
        Self {
            context,
            runtime,
            fixture_name,
            scope,
            param,
            param_index,
            node,
            fixture_stack: PyList::empty(py).unbind(),
        }
    }
}

fn register_with_pytest(py: Python<'_>) -> PyResult<()> {
    let Ok(pytest) = py.import("pytest") else {
        return Ok(());
    };
    pytest
        .getattr("FixtureRequest")?
        .call_method1("register", (py.get_type::<FixtureRequest>(),))?;
    Ok(())
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
        self.node.borrow(py).keywords.clone_ref(py)
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
