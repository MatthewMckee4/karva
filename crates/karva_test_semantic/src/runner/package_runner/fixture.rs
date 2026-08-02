//! Fixture setup, caching, and teardown for package-runner scopes.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use camino::Utf8Path;
use karva_diagnostic::{FixtureFailure, FixtureUsage};
use pyo3::prelude::*;
use pyo3::types::PyIterator;
use ruff_db::diagnostic::Diagnostic;

use crate::diagnostic::fixture_resolution_diagnostic;
use crate::discovery::models::definition::FunctionDefinition;
use crate::extensions::fixtures::{
    Finalizer, FixtureId, FixturePlan, FixtureScope, HasFixtures, NormalizedFixture,
};
use crate::runner::FixtureArguments;
use crate::runner::fixture_resolver::FixturePlanCompiler;
use crate::runner::request::{
    FixtureRequest, RequestContext, RequestFixtureError, RequestMetadata, RequestRuntime,
};
use crate::runner::scoped_storage::ScopeKey;
use crate::runner::test_iterator::FixtureParameter;
use crate::runner::{FinalizerCache, FixtureCache, FixtureCacheKey};
use crate::utils::run_coroutine;

use super::PackageRunner;
use super::failure::TestError;

impl PackageRunner<'_, '_> {
    /// Resolves and runs auto-use fixtures at the start of one scope.
    ///
    /// `current` is the fixture provider for the scope: the session package,
    /// a test module, or a package. Resolution failures
    /// trigger cleanup for the partially initialized scope before returning.
    pub(super) fn run_auto_use_fixtures<'a>(
        &self,
        py: Python<'_>,
        parents: &'a [&'a crate::discovery::DiscoveredPackage],
        current: &'a (dyn HasFixtures<'a> + 'a),
        current_package: &'a Utf8Path,
        scope: FixtureScope,
    ) -> Result<(), TestError> {
        let scope_key = scope_key(scope, current_package);
        let mut compiler = FixturePlanCompiler::new(parents, current, current_package);
        let mut auto_use_fixtures = match compiler.get_normalized_auto_use_fixtures(py, scope) {
            Ok(fixtures) => fixtures,
            Err(error) => {
                return Err(TestError::new(fixture_resolution_diagnostic(error))
                    .with_related(self.clean_up_scope(py, scope_key)));
            }
        };
        let fixture_plan = Rc::new(compiler.finish());
        auto_use_fixtures
            .retain(|fixture_id| !fixture_plan.requires_variant_execution(*fixture_id));

        let executor = Rc::new(FixtureExecutor::new(
            Rc::clone(&fixture_plan),
            Rc::clone(&self.fixture_cache),
            Rc::clone(&self.finalizer_cache),
            Rc::new(HashMap::new()),
            None,
        ));

        let failures = run_fixtures(py, &executor, &auto_use_fixtures, FixtureUsage::AutoUse);
        let Some(failures) = FixtureSetupError::from_vec(failures) else {
            return Ok(());
        };

        Err(failures
            .into_test_error(py, self.context.is_verbose())
            .with_related(self.clean_up_scope(py, scope_key)))
    }

    /// Runs function-scoped finalizers and clears their cached fixture values.
    pub(super) fn clean_up_test_attempt(&self, py: Python<'_>) -> Vec<Diagnostic> {
        self.clean_up_scope(py, ScopeKey::Function)
    }

    /// Clears cached fixture values and runs finalizers for one completed scope.
    pub(super) fn clean_up_scope(&self, py: Python<'_>, scope: ScopeKey<'_>) -> Vec<Diagnostic> {
        let mut diagnostics = Vec::new();
        loop {
            let finalizers = self.finalizer_cache.borrow_mut().take_scope(scope);
            if finalizers.is_empty() {
                break;
            }
            diagnostics.extend(
                finalizers
                    .into_iter()
                    .rev()
                    .filter_map(|finalizer| finalizer.run(py).err()),
            );
        }
        self.fixture_cache.borrow_mut().clear_scope(scope);
        diagnostics
    }

    /// Cleans one scope and promotes teardown failures to run diagnostics.
    pub(super) fn report_scope_cleanup(&mut self, py: Python<'_>, scope: ScopeKey<'_>) {
        for diagnostic in self.clean_up_scope(py, scope) {
            self.state.add_run_diagnostic(diagnostic);
        }
    }

    /// Builds test-call arguments and runs every function-scoped fixture group.
    ///
    /// Finalizers are retained in the scope-aware runner cache until teardown.
    pub(super) fn prepare_test_fixtures(
        &self,
        py: Python<'_>,
        test: &crate::discovery::DiscoveredTestFunction,
        inputs: TestFixtureInputs<'_>,
    ) -> PreparedFixtures {
        let TestFixtureInputs {
            fixture_plan,
            fixture_dependencies,
            use_fixture_dependencies,
            auto_use_fixtures,
            params,
            fixture_params,
            parameter_id,
        } = inputs;
        let test_requests_request = test
            .statement()
            .parameters
            .iter_non_variadic_params()
            .any(|parameter| parameter.parameter.name.as_str() == "request");
        let request_context = if test_requests_request || fixture_plan.uses_request() {
            let fixture_names = request_fixture_names(test, fixture_plan);
            match RequestContext::new(
                py,
                test.py_function.clone_ref(py),
                RequestMetadata {
                    module_name: test.name().module_path().module_name(),
                    path: test.name().module_path().path().as_str(),
                    root_path: self.context.cwd().as_str(),
                    test_name: test.name().function_name(),
                    parameter_id,
                    fixture_names,
                },
            ) {
                Ok(context) => Some(Rc::new(context)),
                Err(error) => {
                    return PreparedFixtures {
                        function_arguments: FixtureArguments::default(),
                        setup_result: Err(FixtureSetupError::from_request_error(test, error)),
                    };
                }
            }
        } else {
            None
        };
        let executor = Rc::new(FixtureExecutor::new(
            Rc::clone(fixture_plan),
            Rc::clone(&self.fixture_cache),
            Rc::clone(&self.finalizer_cache),
            Rc::new(fixture_params),
            request_context,
        ));

        let mut fixture_call_errors = Vec::new();
        let mut function_arguments = FixtureArguments::default();

        for scope in [
            FixtureScope::Session,
            FixtureScope::Package,
            FixtureScope::Module,
            FixtureScope::Function,
        ] {
            fixture_call_errors.extend(run_fixtures_at_scope(
                py,
                &executor,
                auto_use_fixtures,
                scope,
                FixtureUsage::AutoUse,
            ));
            fixture_call_errors.extend(run_fixtures_at_scope(
                py,
                &executor,
                use_fixture_dependencies,
                scope,
                FixtureUsage::UseFixtures,
            ));
            for fixture_id in fixture_dependencies
                .iter()
                .filter(|fixture_id| fixture_plan.fixture(**fixture_id).scope() == scope)
            {
                let fixture = fixture_plan.fixture(*fixture_id);
                match executor.run_fixture(py, *fixture_id) {
                    Ok(value) => {
                        function_arguments
                            .insert(fixture.function_name().to_string(), value.clone_ref(py));
                    }
                    Err(error) => fixture_call_errors.push(PreparedFixtureFailure::new(
                        fixture.function_name(),
                        FixtureUsage::Required,
                        error,
                    )),
                }
            }
        }

        for (key, value) in params {
            function_arguments.insert(
                key,
                Arc::try_unwrap(value).unwrap_or_else(|arc| (*arc).clone_ref(py)),
            );
        }

        if test_requests_request {
            match executor.top_request(py, Rc::clone(test.definition())) {
                Ok(request) => {
                    function_arguments.insert("request".to_string(), request.into_any());
                }
                Err(error) => {
                    fixture_call_errors.push(PreparedFixtureFailure::new(
                        "request",
                        FixtureUsage::Required,
                        FixtureCallError::from_definition(
                            "request",
                            Rc::clone(test.definition()),
                            error,
                            FixtureArguments::default(),
                        ),
                    ));
                }
            }
        }

        PreparedFixtures {
            function_arguments,
            setup_result: FixtureSetupError::from_vec(fixture_call_errors).map_or(Ok(()), Err),
        }
    }
}

/// Immutable fixture roots plus parameter values for one test attempt.
pub(super) struct TestFixtureInputs<'a> {
    pub(super) fixture_plan: &'a Rc<FixturePlan>,
    pub(super) fixture_dependencies: &'a [FixtureId],
    pub(super) use_fixture_dependencies: &'a [FixtureId],
    pub(super) auto_use_fixtures: &'a [FixtureId],
    pub(super) params: HashMap<String, Arc<Py<PyAny>>>,
    pub(super) fixture_params: HashMap<String, FixtureParameter>,
    pub(super) parameter_id: Option<&'a str>,
}

/// Cloneable execution handle used by Python request objects during fixture calls.
struct FixtureExecutor {
    plan: Rc<FixturePlan>,
    fixture_cache: Rc<RefCell<FixtureCache>>,
    finalizer_cache: Rc<RefCell<FinalizerCache>>,
    fixture_params: Rc<HashMap<String, FixtureParameter>>,
    request_context: Option<Rc<RequestContext>>,
    running: RefCell<Vec<FixtureId>>,
}

impl FixtureExecutor {
    fn new(
        plan: Rc<FixturePlan>,
        fixture_cache: Rc<RefCell<FixtureCache>>,
        finalizer_cache: Rc<RefCell<FinalizerCache>>,
        fixture_params: Rc<HashMap<String, FixtureParameter>>,
        request_context: Option<Rc<RequestContext>>,
    ) -> Self {
        Self {
            plan,
            fixture_cache,
            finalizer_cache,
            fixture_params,
            request_context,
            running: RefCell::new(Vec::new()),
        }
    }

    fn cache_key(&self, fixture_id: FixtureId) -> Option<FixtureCacheKey> {
        let fixture = self.plan.fixture(fixture_id);
        if !fixture.is_parameterized() {
            return None;
        }
        let mut parameters = Vec::new();
        let mut visited = HashSet::new();
        self.collect_parameter_indices(fixture_id, &mut visited, &mut parameters);
        parameters.sort_unstable();
        Some(FixtureCacheKey {
            fixture: fixture.name().to_string(),
            parameters,
        })
    }

    fn collect_parameter_indices(
        &self,
        fixture_id: FixtureId,
        visited: &mut HashSet<FixtureId>,
        parameters: &mut Vec<(String, usize)>,
    ) {
        if !visited.insert(fixture_id) {
            return;
        }
        let fixture = self.plan.fixture(fixture_id);
        let name = fixture.name().to_string();
        if let Some(parameter) = self.fixture_params.get(&name) {
            parameters.push((name, parameter.index));
        }
        for dependency in fixture.dependencies() {
            self.collect_parameter_indices(*dependency, visited, parameters);
        }
    }

    #[expect(clippy::result_large_err)]
    fn run_fixture(
        self: &Rc<Self>,
        py: Python<'_>,
        fixture_id: FixtureId,
    ) -> Result<Py<PyAny>, FixtureCallError> {
        let fixture = self.plan.fixture(fixture_id);
        let scope = scope_key(fixture.scope(), fixture.package_owner());
        let cache_key = self.cache_key(fixture_id);
        if let Some(cached) =
            self.fixture_cache
                .borrow()
                .get(py, fixture.function_name(), cache_key.as_ref(), scope)
        {
            return Ok(cached);
        }

        if self.running.borrow().contains(&fixture_id) {
            return Err(FixtureCallError::new(
                fixture,
                pyo3::exceptions::PyRuntimeError::new_err(format!(
                    "recursive dependency involving fixture `{}`",
                    fixture.function_name()
                )),
                FixtureArguments::default(),
            ));
        }
        self.running.borrow_mut().push(fixture_id);
        let result = self.call_fixture(py, fixture_id, cache_key, scope);
        let _ = self.running.borrow_mut().pop();
        result
    }

    #[expect(clippy::result_large_err)]
    fn call_fixture(
        self: &Rc<Self>,
        py: Python<'_>,
        fixture_id: FixtureId,
        cache_key: Option<FixtureCacheKey>,
        scope: ScopeKey<'_>,
    ) -> Result<Py<PyAny>, FixtureCallError> {
        let fixture = self.plan.fixture(fixture_id);
        let mut function_arguments = FixtureArguments::default();

        for dependency_id in fixture.dependencies() {
            let dependency = self.plan.fixture(*dependency_id);
            match self.run_fixture(py, *dependency_id) {
                Ok(value) => {
                    function_arguments
                        .insert(dependency.function_name().to_string(), value.clone_ref(py));
                }
                Err(error) => return Err(error.with_dependent(fixture)),
            }
        }

        if fixture.requests_request() {
            let request = self.fixture_request(py, fixture_id).map_err(|error| {
                FixtureCallError::new(fixture, error, FixtureArguments::default())
            })?;
            function_arguments.insert("request".to_string(), request.into_any());
        }

        let fixture_call_result = match fixture.call(py, &function_arguments) {
            Ok(result) => result,
            Err(error) => return Err(FixtureCallError::new(fixture, error, function_arguments)),
        };
        let (value, finalizer) = match get_value_and_finalizer(py, fixture, fixture_call_result) {
            Ok(result) => result,
            Err(error) => return Err(FixtureCallError::new(fixture, error, function_arguments)),
        };

        self.fixture_cache.borrow_mut().insert(
            fixture.function_name().to_string(),
            cache_key,
            value.clone_ref(py),
            scope,
        );
        if let Some(finalizer) = finalizer {
            self.finalizer_cache.borrow_mut().add_finalizer(finalizer);
        }
        if let Some(context) = &self.request_context {
            context.add_fixture_name(fixture.function_name());
        }
        Ok(value)
    }

    fn fixture_request(
        self: &Rc<Self>,
        py: Python<'_>,
        fixture_id: FixtureId,
    ) -> PyResult<Py<FixtureRequest>> {
        let Some(context) = &self.request_context else {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "fixture request context was not initialized",
            ));
        };
        let fixture = self.plan.fixture(fixture_id);
        let parameter = self.fixture_params.get(&fixture.name().to_string());
        let runtime = self.request_runtime(
            fixture.scope(),
            fixture.package_owner().to_path_buf(),
            Rc::clone(&fixture.definition),
        );
        Py::new(
            py,
            FixtureRequest::new(
                py,
                Rc::clone(context),
                runtime,
                Some(fixture.function_name().to_string()),
                fixture.scope(),
                parameter.map(|parameter| parameter.value.clone_ref(py)),
                Some(parameter.map_or(0, |parameter| parameter.index)),
            )?,
        )
    }

    fn top_request(
        self: &Rc<Self>,
        py: Python<'_>,
        definition: Rc<FunctionDefinition>,
    ) -> PyResult<Py<FixtureRequest>> {
        let Some(context) = &self.request_context else {
            return Err(pyo3::exceptions::PyRuntimeError::new_err(
                "test request context was not initialized",
            ));
        };
        let package_owner = definition
            .name()
            .module_path()
            .path()
            .parent()
            .unwrap_or_else(|| definition.name().module_path().path())
            .to_path_buf();
        let runtime = self.request_runtime(FixtureScope::Function, package_owner, definition);
        Py::new(
            py,
            FixtureRequest::new(
                py,
                Rc::clone(context),
                runtime,
                None,
                FixtureScope::Function,
                None,
                None,
            )?,
        )
    }

    fn request_runtime(
        self: &Rc<Self>,
        requesting_scope: FixtureScope,
        package_owner: camino::Utf8PathBuf,
        definition: Rc<FunctionDefinition>,
    ) -> RequestRuntime {
        let executor = Rc::clone(self);
        let get_fixture = move |py: Python<'_>, name: &str| {
            executor.get_fixture_value(py, name, requesting_scope)
        };

        let finalizer_cache = Rc::clone(&self.finalizer_cache);
        let add_finalizer = move |_py: Python<'_>, callback: Py<PyAny>| {
            finalizer_cache
                .borrow_mut()
                .add_finalizer(Finalizer::callback(
                    callback,
                    requesting_scope,
                    package_owner.clone(),
                    Rc::clone(&definition),
                ));
            Ok(())
        };
        RequestRuntime::new(get_fixture, add_finalizer)
    }

    fn get_fixture_value(
        self: &Rc<Self>,
        py: Python<'_>,
        name: &str,
        requesting_scope: FixtureScope,
    ) -> Result<Py<PyAny>, RequestFixtureError> {
        let Some(fixture_id) = self.plan.dynamic_fixture(name) else {
            return Err(RequestFixtureError::Lookup(format!(
                "fixture {name:?} not found"
            )));
        };
        let fixture = self.plan.fixture(fixture_id);
        if !requesting_scope.can_use(fixture.scope()) {
            return Err(RequestFixtureError::Lookup(format!(
                "ScopeMismatch: {}-scoped request cannot access {}-scoped fixture {name:?}",
                requesting_scope.name(),
                fixture.scope().name(),
            )));
        }
        if fixture.parameters().is_some()
            && !self
                .fixture_params
                .contains_key(&fixture.name().to_string())
        {
            return Err(RequestFixtureError::Lookup(format!(
                "requested parametrized fixture {name:?} has no parameter for this test"
            )));
        }
        if let Some(context) = &self.request_context {
            context.add_fixture_name(name);
        }
        self.run_fixture(py, fixture_id)
            .map_err(|error| RequestFixtureError::Python(error.error))
    }
}

fn request_fixture_names(
    test: &crate::discovery::DiscoveredTestFunction,
    fixture_plan: &FixturePlan,
) -> Vec<String> {
    let mut seen = HashSet::new();
    test.statement()
        .parameters
        .iter_non_variadic_params()
        .map(|parameter| parameter.parameter.name.as_str().to_string())
        .chain(fixture_plan.fixture_names().map(str::to_string))
        .filter(|name| seen.insert(name.clone()))
        .collect()
}

fn run_fixtures<'a>(
    py: Python<'_>,
    executor: &Rc<FixtureExecutor>,
    fixture_ids: impl IntoIterator<Item = &'a FixtureId>,
    usage: FixtureUsage,
) -> Vec<PreparedFixtureFailure> {
    let mut errors = Vec::new();
    for fixture_id in fixture_ids {
        let fixture = executor.plan.fixture(*fixture_id);
        if let Err(error) = executor.run_fixture(py, *fixture_id) {
            errors.push(PreparedFixtureFailure::new(
                fixture.function_name(),
                usage,
                error,
            ));
        }
    }
    errors
}

fn run_fixtures_at_scope(
    py: Python<'_>,
    executor: &Rc<FixtureExecutor>,
    fixture_ids: &[FixtureId],
    scope: FixtureScope,
    usage: FixtureUsage,
) -> Vec<PreparedFixtureFailure> {
    run_fixtures(
        py,
        executor,
        fixture_ids
            .iter()
            .filter(|fixture_id| executor.plan.fixture(**fixture_id).scope() == scope),
        usage,
    )
}

fn scope_key(scope: FixtureScope, package_owner: &Utf8Path) -> ScopeKey<'_> {
    match scope {
        FixtureScope::Session => ScopeKey::Session,
        FixtureScope::Package => ScopeKey::Package(package_owner),
        FixtureScope::Module => ScopeKey::Module,
        FixtureScope::Function => ScopeKey::Function,
    }
}

/// Fixture state prepared for one test attempt.
pub(super) struct PreparedFixtures {
    /// Keyword arguments passed to the Python test function.
    pub(super) function_arguments: FixtureArguments,
    /// Whether fixture setup completed or blocked the test call.
    pub(super) setup_result: Result<(), FixtureSetupError>,
}

/// Raw fixture call error paired with its relationship to the blocked test.
struct PreparedFixtureFailure {
    /// Python call failure and source context for diagnostic rendering.
    error: FixtureCallError,

    /// Requested fixture and dependency chain exposed to result consumers.
    fixture_failure: FixtureFailure,
}

/// One or more fixture failures from a single setup phase.
///
/// Splitting the first failure from the remainder makes the primary diagnostic
/// invariant explicit and removes unchecked indexing from error reporting.
pub(super) struct FixtureSetupError {
    /// Failure promoted to the primary diagnostic.
    first: PreparedFixtureFailure,

    /// Additional failures produced during the same setup phase.
    related: Vec<PreparedFixtureFailure>,
}

impl FixtureSetupError {
    fn from_vec(failures: Vec<PreparedFixtureFailure>) -> Option<Self> {
        let mut failures = failures.into_iter();
        Some(Self {
            first: failures.next()?,
            related: failures.collect(),
        })
    }

    fn from_request_error(test: &crate::discovery::DiscoveredTestFunction, error: PyErr) -> Self {
        let failure = FixtureCallError::from_definition(
            "request",
            Rc::clone(test.definition()),
            error,
            FixtureArguments::default(),
        );
        Self {
            first: PreparedFixtureFailure::new("request", FixtureUsage::Required, failure),
            related: Vec::new(),
        }
    }

    /// Renders raw Python failures into one test-owned execution error.
    pub(super) fn into_test_error(self, py: Python<'_>, verbose: bool) -> TestError {
        let (diagnostic, fixture_failure) = render_fixture_failure(py, self.first, verbose);
        let mut related = Vec::with_capacity(self.related.len());
        let mut fixture_failures = Vec::with_capacity(self.related.len() + 1);
        fixture_failures.push(fixture_failure);
        for failure in self.related {
            let (diagnostic, fixture_failure) = render_fixture_failure(py, failure, verbose);
            related.push(diagnostic);
            fixture_failures.push(fixture_failure);
        }
        TestError::from_fixture_failures(diagnostic, related, fixture_failures)
    }
}

impl PreparedFixtureFailure {
    fn new(requested_fixture: &str, usage: FixtureUsage, error: FixtureCallError) -> Self {
        let mut dependency_chain = vec![requested_fixture.to_string()];
        dependency_chain.extend(
            error
                .dependency_chain
                .iter()
                .rev()
                .map(|entry| entry.name.clone())
                .filter(|name| name != requested_fixture),
        );
        if dependency_chain
            .last()
            .is_none_or(|name| name != &error.fixture_name)
        {
            dependency_chain.push(error.fixture_name.clone());
        }
        Self {
            fixture_failure: FixtureFailure::new(
                requested_fixture.to_string(),
                usage,
                dependency_chain,
            ),
            error,
        }
    }
}

fn render_fixture_failure(
    py: Python<'_>,
    failure: PreparedFixtureFailure,
    verbose: bool,
) -> (Diagnostic, FixtureFailure) {
    (
        crate::diagnostic::fixture_failure_diagnostic(py, failure.error, verbose),
        failure.fixture_failure,
    )
}

/// Failure raised while preparing or calling a fixture.
pub struct FixtureCallError {
    /// Name of the fixture that directly failed.
    pub(crate) fixture_name: String,
    /// Python exception raised by fixture setup.
    pub(crate) error: PyErr,
    /// Immutable fixture identity, syntax, and source.
    pub(crate) definition: Rc<FunctionDefinition>,
    /// Arguments already prepared for the failing fixture.
    pub(crate) arguments: FixtureArguments,
    /// Intermediate fixtures between the requested fixture and failing fixture.
    ///
    /// Entries are appended bottom-up while the error unwinds.
    pub(crate) dependency_chain: Vec<FixtureChainEntry>,
}

/// Intermediate fixture in a dependency path leading to a setup failure.
pub struct FixtureChainEntry {
    /// Fixture name shown in dependency diagnostics.
    pub(crate) name: String,
    /// Immutable fixture identity, syntax, and source.
    pub(crate) definition: Rc<FunctionDefinition>,
}

impl FixtureCallError {
    fn new(fixture: &NormalizedFixture, error: PyErr, arguments: FixtureArguments) -> Self {
        Self::from_definition(
            fixture.function_name(),
            Rc::clone(&fixture.definition),
            error,
            arguments,
        )
    }

    fn from_definition(
        fixture_name: &str,
        definition: Rc<FunctionDefinition>,
        error: PyErr,
        arguments: FixtureArguments,
    ) -> Self {
        Self {
            fixture_name: fixture_name.to_string(),
            error,
            definition,
            arguments,
            dependency_chain: Vec::new(),
        }
    }

    #[must_use]
    fn with_dependent(mut self, fixture: &NormalizedFixture) -> Self {
        self.dependency_chain.push(FixtureChainEntry {
            name: fixture.function_name().to_string(),
            definition: Rc::clone(&fixture.definition),
        });
        self
    }
}

/// Extracts the value and teardown finalizer produced by a fixture call.
fn get_value_and_finalizer(
    py: Python<'_>,
    fixture: &NormalizedFixture,
    fixture_call_result: Py<PyAny>,
) -> PyResult<(Py<PyAny>, Option<Finalizer>)> {
    if fixture.is_generator && fixture.statement().is_async {
        let bound = fixture_call_result.bind(py);
        let anext_coroutine = bound.call_method0("__anext__")?;
        let value = run_coroutine(py, anext_coroutine.unbind())?;

        let finalizer = Finalizer::generator(
            fixture_call_result,
            true,
            fixture.scope(),
            fixture.package_owner().to_path_buf(),
            Rc::clone(&fixture.definition),
        );

        Ok((value, Some(finalizer)))
    } else if fixture.is_generator {
        let mut bound_iterator = fixture_call_result
            .into_bound(py)
            .cast_into::<PyIterator>()
            .map_err(|_| {
                pyo3::exceptions::PyTypeError::new_err(format!(
                    "Generator fixture `{}` did not return an iterator",
                    fixture.function_name()
                ))
            })?;
        match bound_iterator.next() {
            Some(Ok(value)) => {
                let finalizer = Finalizer::generator(
                    bound_iterator.clone().unbind().into_any(),
                    false,
                    fixture.scope(),
                    fixture.package_owner().to_path_buf(),
                    Rc::clone(&fixture.definition),
                );

                Ok((value.unbind(), Some(finalizer)))
            }
            Some(Err(error)) => Err(error),
            None => Err(pyo3::exceptions::PyRuntimeError::new_err(
                "Generator fixture yielded no value",
            )),
        }
    } else {
        Ok((fixture_call_result, None))
    }
}
