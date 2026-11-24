//! Owns Python request context across reentrant fixture lookup and teardown.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use karva_diagnostic::{DiagnosticFormat, DisplayDiagnosticConfig, render_diagnostic};
use karva_python_semantic::QualifiedFunctionName;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyAnyMethods, PyDict, PyModule};

use crate::Context;
use crate::extensions::fixtures::{
    FinalizerCallbacks, FixtureDependency, FixtureIdentity, FixturePlan, FixtureScope,
    NormalizedFixture,
};
use crate::extensions::tags::Tags;
use crate::extensions::tags::parametrize::FixtureParameter;
use crate::extensions::tags::python::PyTestFunction;
use crate::runner::scoped_storage::{ScopeKey, ScopedStorage};

use super::PackageRunner;
use super::fixture::{FixtureInputs, FixtureRuntime, scope_key};
use super::variant::VariantInput;

pub(super) type Callbacks = Rc<FinalizerCallbacks>;

/// Python nodes shared by all requests owned by one worker and fixture lifetime.
#[derive(Default)]
pub(super) struct RequestNodes(RefCell<ScopedStorage<Option<Py<PyAny>>>>);

impl RequestNodes {
    fn get(&self, py: Python<'_>, key: ScopeKey<'_>) -> Option<Py<PyAny>> {
        self.0
            .borrow()
            .with(key, |node| node.as_ref().map(|node| node.clone_ref(py)))
            .flatten()
    }

    fn node(
        &self,
        py: Python<'_>,
        key: ScopeKey<'_>,
        make: impl FnOnce() -> PyResult<Py<PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        if let Some(node) = self.get(py, key) {
            return Ok(node);
        }
        let node = make()?;
        self.0
            .borrow_mut()
            .with_mut(key, |slot| *slot = Some(node.clone_ref(py)));
        Ok(node)
    }

    pub(super) fn clear(&self, key: ScopeKey<'_>) {
        let node = self.0.borrow_mut().take(key);
        drop(node);
    }
}

impl PackageRunner<'_, '_> {
    pub(super) fn inherited_request_tags(
        &self,
        py: Python<'_>,
        input: &VariantInput<'_>,
    ) -> PyResult<Tags> {
        let keys = std::iter::once(ScopeKey::Module(&input.module_path))
            .chain(
                self.active_packages
                    .iter()
                    .rev()
                    .map(|path| ScopeKey::Package(path)),
            )
            .chain(std::iter::once(ScopeKey::Session));
        let Some(node) = keys
            .into_iter()
            .find_map(|key| self.fixtures.nodes.get(py, key))
        else {
            return Ok(Tags::default());
        };
        read_runtime_tags(py, &node, input.test.py_function.bind(py))
    }
}

/// Test metadata and resolved fixture names for an individual attempt.
pub(super) struct RequestContext {
    runtime: Weak<FixtureRuntime>,
    plan: Rc<FixturePlan>,
    test: QualifiedFunctionName,
    session: Py<PyAny>,
    packages: HashMap<Utf8PathBuf, Py<PyAny>>,
    module_node: Py<PyAny>,
    test_node: Py<PyAny>,
    module: Option<Py<PyAny>>,
    path: Py<PyAny>,
    module_owner: Utf8PathBuf,
    names: RefCell<Vec<String>>,
    pub(super) indirect: HashMap<String, FixtureParameter>,
    direct: HashMap<String, Arc<Py<PyAny>>>,
}

impl RequestContext {
    pub(super) fn new(
        py: Python<'_>,
        runtime: &Rc<FixtureRuntime>,
        run: &Context<'_>,
        package_paths: &[Utf8PathBuf],
        input: &VariantInput<'_>,
    ) -> PyResult<Rc<Self>> {
        let plan = &input.fixtures.plan;
        let cwd = run.cwd();
        let test = input.test;
        let module_path = &input.module_path;
        let api = request_module(py)?;
        let path_type = py.import("pathlib")?.getattr("Path")?;
        let rootpath = path_type.call1((cwd.as_str(),))?;
        let node_type = api.getattr("RequestNode")?;
        let session = runtime.nodes.node(py, ScopeKey::Session, || {
            let json = serde_json::to_string(run.settings())
                .map_err(|error| PyRuntimeError::new_err(error.to_string()))?;
            let settings_dict = py.import("json")?.call_method1("loads", (json,))?;
            let config = api
                .getattr("RequestConfig")?
                .call1((&rootpath, settings_dict))?;
            Ok(node_type
                .call1(("", "", &rootpath, py.None(), config))?
                .unbind())
        })?;
        let config = session.getattr(py, "config")?;
        let mut packages = HashMap::new();
        let mut parent = session.clone_ref(py);
        for path in package_paths {
            let package = runtime.nodes.node(py, ScopeKey::Package(path), || {
                let path_id = path.strip_prefix(cwd).unwrap_or(path);
                Ok(node_type
                    .call1((
                        path.file_name().unwrap_or_default(),
                        path_id.as_str(),
                        path_type.call1((cwd.join(path).as_str(),))?,
                        &parent,
                        &config,
                    ))?
                    .unbind())
            })?;
            parent = package.clone_ref(py);
            packages.insert(path.clone(), package);
        }
        let module = py
            .import("sys")?
            .getattr("modules")?
            .get_item(test.name().module_path().module_name())
            .ok();
        let path = path_type.call1((cwd.join(module_path).as_str(),))?;
        let relative_path = module_path.strip_prefix(cwd).unwrap_or(module_path);
        let module_node = runtime.nodes.node(py, ScopeKey::Module(module_path), || {
            let node = node_type.call1((
                module_path.file_name().unwrap_or_default(),
                relative_path.as_str(),
                &path,
                &parent,
                &config,
                &module,
            ))?;
            if let Some(module) = &module
                && let Ok(markers) = module.getattr("pytestmark")
            {
                add_static_markers(&node, markers)?;
            }
            Ok(node.unbind())
        })?;
        let name = test.name().function_name();
        let name = input
            .identity
            .request_id
            .as_ref()
            .map_or_else(|| name.to_owned(), |id| format!("{name}[{id}]"));
        let nodeid = format!("{relative_path}::{name}");
        let function = test.py_function.bind(py);
        let underlying = function
            .cast::<PyTestFunction>()
            .ok()
            .map(|function| function.borrow().function.clone_ref(py));
        let node = node_type.call1((name, nodeid, &path, &module_node, &config, function))?;
        if let Ok(markers) = underlying
            .as_ref()
            .map_or(function, |function| function.bind(py))
            .getattr("pytestmark")
        {
            add_static_markers(&node, markers)?;
        }
        for marker in &input.case_markers {
            node.call_method1("_add_marker", (marker.bind(py),))?;
        }
        let keywords = node.getattr("keywords")?;
        for name in input.tags.tag_names() {
            if !keywords.contains(name)? {
                keywords.set_item(name, true)?;
            }
        }
        let mut names = input.params.keys().cloned().collect::<Vec<_>>();
        names.sort_unstable();
        let mut seen = HashSet::new();
        let mut pending = input
            .fixtures
            .auto_use
            .iter()
            .chain(&*input.fixtures.use_dependencies)
            .chain(&*input.fixtures.dependencies)
            .copied()
            .collect::<Vec<_>>();
        pending.reverse();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            let fixture = plan.fixture(id);
            names.push(fixture.function_name().to_owned());
            if fixture.takes_request() && !names.iter().any(|name| name == "request") {
                names.push("request".to_owned());
            }
            pending.extend(
                fixture
                    .dependencies()
                    .iter()
                    .rev()
                    .filter_map(FixtureDependency::fixture_id),
            );
        }
        if test.required_fixtures().contains(&"request")
            && !names.iter().any(|name| name == "request")
        {
            names.push("request".to_owned());
        }
        Ok(Rc::new(Self {
            runtime: Rc::downgrade(runtime),
            plan: Rc::clone(plan),
            test: test.name().clone(),
            session,
            packages,
            module_node,
            test_node: node.unbind(),
            module: module.map(Bound::unbind),
            path: path.unbind(),
            module_owner: module_path.clone(),
            names: RefCell::new(names),
            indirect: input.indirect.clone(),
            direct: input.params.clone(),
        }))
    }

    pub(super) fn request(
        self: &Rc<Self>,
        py: Python<'_>,
        owner: Option<FixtureIdentity>,
        scope: FixtureScope,
        package: &Utf8Path,
        callbacks: Callbacks,
        param: Option<&Py<PyAny>>,
    ) -> PyResult<Py<PyAny>> {
        let node = match scope {
            FixtureScope::Session => &self.session,
            FixtureScope::Package => self.packages.get(package).unwrap_or(&self.session),
            FixtureScope::Module => &self.module_node,
            FixtureScope::Function => &self.test_node,
        };
        let state = Py::new(
            py,
            FixtureRequestState {
                context: Rc::clone(self),
                owner,
                scope,
                callbacks,
            },
        )?;
        let request = request_module(py)?.getattr("FixtureRequest")?.call1((
            state,
            node,
            &self.test_node,
            &self.module,
            &self.path,
        ))?;
        if let Some(param) = param {
            request.setattr("param", param)?;
        }
        Ok(request.unbind())
    }

    pub(super) fn fixture_scope(&self, fixture: &NormalizedFixture) -> FixtureScope {
        self.indirect
            .get(fixture.function_name())
            .and_then(|param| param.scope)
            .unwrap_or_else(|| fixture.scope())
    }

    pub(super) fn record_fixture_name(&self, name: &str) {
        let mut names = self.names.borrow_mut();
        if !names.iter().any(|existing| existing == name) {
            names.push(name.to_owned());
        }
    }

    pub(super) fn runtime_tags(&self, py: Python<'_>) -> PyResult<Tags> {
        let function = self.test_node.bind(py).getattr("obj")?;
        read_runtime_tags(py, &self.test_node, &function)
    }
}

/// Callbacks use owned state so Python can resolve fixtures while Rust is in a call.
#[pyclass(unsendable, module = "karva._karva", name = "_FixtureRequestState")]
pub struct FixtureRequestState {
    context: Rc<RequestContext>,
    owner: Option<FixtureIdentity>,
    scope: FixtureScope,
    callbacks: Callbacks,
}

#[pymethods]
impl FixtureRequestState {
    #[getter]
    fn scope(&self) -> &'static str {
        self.scope.name()
    }

    #[getter]
    fn fixturename(&self) -> Option<&str> {
        self.owner
            .as_ref()
            .map(|owner| owner.name().function_name())
    }

    #[getter]
    fn fixturenames(&self) -> Vec<String> {
        self.context.names.borrow().clone()
    }

    fn addfinalizer(&self, callback: Py<PyAny>, py: Python<'_>) -> PyResult<()> {
        self.callbacks.register(py, callback)
    }

    fn getfixturevalue(&self, py: Python<'_>, name: &str) -> PyResult<Py<PyAny>> {
        let context = &self.context;
        if !self.callbacks.is_active() {
            return Err(lookup_error(py, "fixture request has ended"));
        }
        let runtime = context
            .runtime
            .upgrade()
            .ok_or_else(|| lookup_error(py, "fixture request has ended"))?;
        let attempt = runtime.active_request.borrow().upgrade();
        let origin = runtime.override_origin(name);
        let origin = origin.as_ref().or(self.owner.as_ref());
        let original = context.plan.dynamic_fixture(&context.test, name, origin);
        let current = attempt.as_ref().and_then(|attempt| {
            let candidate = attempt
                .plan
                .dynamic_fixture(&attempt.test, name, origin)
                .filter(|candidate| {
                    original.is_some_and(|original| candidate.name == original.name)
                });
            let shares_context = attempt.module_owner == context.module_owner
                || (matches!(self.scope, FixtureScope::Package | FixtureScope::Session)
                    && candidate.is_some());
            shares_context.then_some((attempt, candidate))
        });
        if let Some(value) = current
            .map_or(context, |(attempt, _)| attempt)
            .direct
            .get(name)
        {
            if self.scope != FixtureScope::Function {
                return Err(lookup_error(
                    py,
                    &format!(
                        "ScopeMismatch: {}-scoped request cannot access function-scoped parameter '{name}'",
                        self.scope.name(),
                    ),
                ));
            }
            return Ok(value.clone_ref(py));
        }
        let current = current
            .and_then(|(attempt, candidate)| candidate.map(|candidate| (attempt, candidate)));
        let (selected, resolution) = current.map_or_else(
            || (context, original),
            |(attempt, candidate)| (attempt, Some(candidate)),
        );
        let id = match resolution.map(|fixture| &fixture.resolution) {
            Some(Ok(id)) => *id,
            Some(Err(diagnostic)) => {
                let rendered = render_diagnostic(
                    diagnostic,
                    Utf8Path::new(""),
                    DisplayDiagnosticConfig::new(DiagnosticFormat::Concise, false),
                );
                return Err(lookup_error(py, rendered.message()));
            }
            None => return Err(lookup_error(py, &format!("fixture '{name}' not found"))),
        };
        let fixture = selected.plan.fixture(id);
        if !self.scope.can_use(fixture.scope()) {
            return Err(lookup_error(
                py,
                &format!(
                    "ScopeMismatch: {}-scoped request cannot access {}-scoped fixture '{name}'",
                    self.scope.name(),
                    fixture.scope().name(),
                ),
            ));
        }
        let module = Some(selected.module_owner.as_path());
        let scope = scope_key(
            selected.fixture_scope(fixture),
            fixture.package_owner(),
            module,
        );
        let cached = || runtime.cached_fixture(py, fixture, scope);
        if runtime.tearing_down.get() {
            return cached().ok_or_else(|| {
                lookup_error(
                    py,
                    &format!("fixture '{name}' is unavailable during teardown"),
                )
            });
        }
        if current.is_none()
            && let Some(value) = cached()
        {
            return Ok(value);
        }
        runtime
            .run_fixture(
                py,
                &selected.plan,
                id,
                FixtureInputs {
                    module_owner: module,
                    request: Some(selected),
                    direct: Some(&selected.direct),
                },
            )
            .map_err(|error| error.error)
    }
}

pub(super) fn lookup_error(py: Python<'_>, message: &str) -> PyErr {
    request_module(py)
        .and_then(|module| module.getattr("_lookup_error"))
        .and_then(|class| class.call1((message,)))
        .map_or_else(|error| error, PyErr::from_value)
}

fn read_runtime_tags(
    py: Python<'_>,
    node: &Py<PyAny>,
    function: &Bound<'_, PyAny>,
) -> PyResult<Tags> {
    let marks = node.call_method0(py, "_all_runtime_markers")?;
    let underlying = function
        .cast::<PyTestFunction>()
        .ok()
        .map(|function| function.borrow().function.clone_ref(py));
    let globals = underlying
        .as_ref()
        .map_or(function, |function| function.bind(py))
        .getattr("__globals__")
        .ok()
        .and_then(|globals| globals.cast_into::<PyDict>().ok());
    Tags::from_request_marks(marks.bind(py), globals.as_ref())
}

fn add_static_markers(node: &Bound<'_, PyAny>, markers: Bound<'_, PyAny>) -> PyResult<()> {
    if let Ok(markers) = markers.try_iter() {
        for marker in markers {
            node.call_method1("_add_marker", (marker?,))?;
        }
    } else {
        node.call_method1("_add_marker", (markers,))?;
    }
    Ok(())
}

fn request_module(py: Python<'_>) -> PyResult<Bound<'_, PyModule>> {
    py.import("karva._fixtures.request")
}
