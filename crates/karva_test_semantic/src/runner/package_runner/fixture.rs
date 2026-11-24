//! Fixture setup, caching, and teardown for package-runner scopes.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use karva_diagnostic::{Diagnostic, FixtureFailure, FixtureUsage, Severity, TestExecutionOutcome};
use pyo3::prelude::*;
use pyo3::types::PyIterator;

use crate::diagnostic::fixture_resolution_diagnostic;
use crate::discovery::models::definition::FunctionDefinition;
use crate::extensions::fixtures::{
    Finalizer, FixtureDependency, FixtureId, FixtureIdentity, FixturePlan, FixtureScope,
    HasFixtures, NormalizedFixture,
};
use crate::extensions::tags::parametrize::FixtureParameter;
use crate::extensions::tags::skip::{extract_skip_reason, is_skip_exception};
use crate::runner::fixture_resolver::FixturePlanCompiler;
use crate::runner::scoped_storage::ScopeKey;
use crate::runner::{FixtureArguments, FixtureCache};
use crate::utils::run_coroutine;

use super::PackageRunner;
use super::failure::TestError;
use super::request::{Callbacks, RequestContext, RequestNodes, lookup_error};

impl PackageRunner<'_, '_> {
    /// Resolves and runs auto-use fixtures at the start of one scope.
    ///
    /// `current` is the fixture provider for the scope: the session package,
    /// a test module, or a package. Resolution failures
    /// trigger cleanup for the partially initialized scope before returning.
    pub(super) fn run_auto_use_fixtures<'a>(
        &'a self,
        py: Python<'_>,
        parents: &'a [&'a crate::discovery::DiscoveredPackage],
        current: &'a (dyn HasFixtures<'a> + 'a),
        current_package: &'a Utf8Path,
        scope: FixtureScope,
    ) -> Result<(), TestError> {
        let module_owner = self.active_module.clone();
        let scope_key = scope_key(scope, current_package, module_owner.as_deref());
        let mut compiler =
            FixturePlanCompiler::new(parents, current, current_package, &self.indirect_fixtures);
        let mut auto_use_fixtures = match compiler.get_normalized_auto_use_fixtures(py, scope) {
            Ok(fixtures) => fixtures,
            Err(error) => {
                return Err(TestError::new(fixture_resolution_diagnostic(error))
                    .with_related(self.clean_up_scope(py, scope_key)));
            }
        };
        auto_use_fixtures.retain(|id| !compiler.fixture(*id).needs_test_context());
        let fixture_plan = compiler.finish(py);

        // Commit the preceding result before a broader-scope fixture can
        // terminate the worker. Empty scopes leave delivery to the next test
        // checkpoint, which combines both frames in one socket flush.
        if !auto_use_fixtures.is_empty() {
            self.context.flush_test_results();
        }
        let failures = self.fixtures.run_fixtures(
            py,
            &fixture_plan,
            &auto_use_fixtures,
            FixtureUsage::AutoUse,
            FixtureInputs {
                module_owner: module_owner.as_deref(),
                ..FixtureInputs::default()
            },
        );
        let Some(failures) = FixtureSetupError::from_vec(failures) else {
            return Ok(());
        };

        Err(failures
            .into_test_error(py, self.context.is_verbose())
            .with_related(self.clean_up_scope(py, scope_key)))
    }

    /// Runs function-scoped finalizers and clears their cached fixture values.
    pub(super) fn clean_up_test_attempt(
        &self,
        py: Python<'_>,
        callbacks: Option<Callbacks>,
    ) -> Vec<Diagnostic> {
        self.fixtures.tearing_down.set(true);
        let mut diagnostics = Vec::new();
        if let Some(callbacks) = callbacks {
            callbacks.run(py, |error| {
                diagnostics.push(Diagnostic::new(
                    "fixture-finalizer",
                    Severity::Error,
                    format!("Test finalizer failed: {error}"),
                ));
            });
        }
        diagnostics.extend(self.clean_up_scope(py, ScopeKey::Function));
        self.fixtures.active_request.take();
        diagnostics
    }

    /// Releases cached fixture instances in reverse setup order.
    fn clean_up_scope(&self, py: Python<'_>, scope: ScopeKey<'_>) -> Vec<Diagnostic> {
        self.fixtures.tearing_down.set(true);
        let order = self.fixtures.cache.borrow_mut().take_order(scope);
        for fixture in order
            .into_iter()
            .rev()
            .filter_map(|fixture| fixture.upgrade())
        {
            self.fixtures.retire(py, &fixture);
        }
        self.fixtures.cache.borrow_mut().clear(scope);
        self.fixtures.nodes.clear(scope);
        self.fixtures.tearing_down.set(false);
        self.fixtures.cleanup_diagnostics.take()
    }

    /// Cleans one scope and promotes teardown failures to run diagnostics.
    pub(super) fn report_scope_cleanup(&mut self, py: Python<'_>, scope: ScopeKey<'_>) {
        if self.fixtures.cache.borrow().has_values(scope) {
            self.context.flush_test_results();
        }
        for diagnostic in self.clean_up_scope(py, scope) {
            self.state.add_run_diagnostic(diagnostic);
        }
    }

    /// Builds test-call arguments and runs every function-scoped fixture group.
    pub(super) fn prepare_test_fixtures(
        &self,
        py: Python<'_>,
        input: &super::variant::VariantInput<'_>,
    ) -> PreparedFixtures {
        let fixture_plan = &input.fixtures.plan;
        let takes_request = input.test.required_fixtures().contains(&"request")
            && !input.params.contains_key("request");
        let uses_request = takes_request
            || (self.fixtures.has_retained_requests.get()
                && fixture_plan.has_dynamic_parameters(input.test.name()))
            || !input.indirect.is_empty()
            || input
                .fixtures
                .dependencies
                .iter()
                .chain(&*input.fixtures.use_dependencies)
                .chain(&*input.fixtures.auto_use)
                .any(|id| fixture_plan.fixture(*id).needs_test_context());
        let request = uses_request
            .then(|| {
                RequestContext::new(
                    py,
                    &self.fixtures,
                    self.context,
                    &self.active_packages,
                    input,
                )
            })
            .transpose();
        let request = match request {
            Ok(request) => request,
            Err(error) => {
                return PreparedFixtures {
                    function_arguments: FixtureArguments::default(),
                    setup_result: Err(request_setup_error(&error)),
                    skipped_setup: None,
                    request: None,
                    test_callbacks: None,
                };
            }
        };
        if let Some(request) = &request {
            self.fixtures.active_request.replace(Rc::downgrade(request));
        }
        let active_request = request.as_ref();
        let fixture_inputs = FixtureInputs {
            module_owner: self.active_module.as_deref(),
            request: active_request,
            direct: Some(&input.params),
        };
        let mut requests: Vec<_> = input
            .fixtures
            .auto_use
            .iter()
            .map(|&id| (id, FixtureUsage::AutoUse))
            .chain(
                input
                    .fixtures
                    .use_dependencies
                    .iter()
                    .map(|&id| (id, FixtureUsage::UseFixtures)),
            )
            .chain(
                input
                    .fixtures
                    .dependencies
                    .iter()
                    .map(|&id| (id, FixtureUsage::Required)),
            )
            .collect();
        let mut fixture_call_errors = Vec::new();
        let mut function_arguments = FixtureArguments::default();
        let mut setup_scopes_by_fixture = HashMap::new();

        // Run broader dependencies before function-scoped autouse fixtures.
        let has_function_auto_use = input.fixtures.auto_use.iter().any(|&id| {
            fixture_setup_order(
                fixture_plan,
                id,
                active_request,
                &mut setup_scopes_by_fixture,
            )
            .scope
                == FixtureScope::Function
        });
        let setup_scopes = if has_function_auto_use {
            FixtureScope::Function.scopes_above()
        } else {
            &[FixtureScope::Function]
        };
        for &scope in setup_scopes.iter().rev() {
            let mut pass = SetupPass {
                scope,
                visited: HashSet::new(),
                setup_scopes: &mut setup_scopes_by_fixture,
            };
            requests.retain(|&(id, usage)| {
                let fixture = fixture_plan.fixture(id);
                let run_request = scope == FixtureScope::Function
                    || fixture_setup_order(fixture_plan, id, active_request, pass.setup_scopes)
                        .scope
                        == scope;
                let result = if run_request {
                    self.fixtures
                        .run_fixture(py, fixture_plan, id, fixture_inputs)
                        .map(|value| {
                            if usage == FixtureUsage::Required {
                                function_arguments
                                    .insert(fixture.function_name().to_string(), value);
                            }
                        })
                } else {
                    self.fixtures.prepare_dependencies_at_scope(
                        py,
                        fixture_plan,
                        id,
                        &mut pass,
                        fixture_inputs,
                    )
                };
                if let Err(error) = result {
                    fixture_call_errors.push(PreparedFixtureFailure::new(
                        fixture.function_name(),
                        usage,
                        error,
                    ));
                    return false;
                }
                !run_request
            });
        }

        let parameters = input.test.parameters();
        for (name, value) in &input.params {
            if parameters.is_none_or(|parameters| parameters.index(name).is_some()) {
                function_arguments.insert(name.clone(), value.clone_ref(py));
            }
        }

        let test_callbacks = takes_request.then(Callbacks::default);
        let test_request = if let (Some(request), Some(callbacks)) = (&request, &test_callbacks) {
            request
                .request(
                    py,
                    None,
                    FixtureScope::Function,
                    Utf8Path::new(""),
                    Rc::clone(callbacks),
                    None,
                )
                .map(|value| {
                    function_arguments.insert("request".to_owned(), value);
                })
        } else {
            Ok(())
        };
        let setup = FixtureSetupError::from_vec(fixture_call_errors);
        let skipped_setup = setup.as_ref().and_then(|error| error.skip_outcome(py));
        let setup_result = test_request
            .map_err(|error| request_setup_error(&error))
            .and_then(|()| {
                setup.map_or(Ok(()), |error| {
                    Err(error.into_test_error(py, self.context.is_verbose()))
                })
            });
        PreparedFixtures {
            function_arguments,
            setup_result,
            skipped_setup,
            request,
            test_callbacks,
        }
    }
}

/// Scope caches and active setup stack shared with Python dynamic requests.
#[derive(Default)]
pub(super) struct FixtureRuntime {
    cache: RefCell<FixtureCache>,
    active: RefCell<Vec<FixtureSetup>>,
    cleanup_diagnostics: RefCell<Vec<Diagnostic>>,
    pub(super) nodes: RequestNodes,
    pub(super) tearing_down: Cell<bool>,
    /// A broader fixture has received a request that can outlive its first test.
    has_retained_requests: Cell<bool>,
    /// Current attempt, used to resolve parameters for retained fixture requests.
    pub(super) active_request: RefCell<Weak<RequestContext>>,
}

struct FixtureSetup {
    identity: FixtureIdentity,
    dependencies: Vec<Rc<FixtureInstance>>,
}

/// One cached setup result and the dependency edges governing its teardown.
pub(in crate::runner) struct FixtureInstance {
    identity: FixtureIdentity,
    scope: FixtureScope,
    package_owner: Utf8PathBuf,
    module_owner: Option<Utf8PathBuf>,
    parameter: Option<FixtureParameter>,
    result: Result<Py<PyAny>, Box<FixtureCallError>>,
    has_parameters: bool,
    finalizer: RefCell<Option<Finalizer>>,
    dependencies: Vec<Weak<Self>>,
    dependents: RefCell<Vec<Weak<Self>>>,
    retired: Cell<bool>,
}

impl FixtureInstance {
    pub(in crate::runner) fn identity(&self) -> &FixtureIdentity {
        &self.identity
    }

    pub(in crate::runner) fn scope_key(&self) -> ScopeKey<'_> {
        scope_key(
            self.scope,
            &self.package_owner,
            self.module_owner.as_deref(),
        )
    }

    /// A different case can invalidate a fixture through a transitive dynamic dependency.
    fn matches_parameters(&self, py: Python<'_>, request: Option<&RequestContext>) -> bool {
        if !self.has_parameters && request.is_none_or(|request| request.indirect.is_empty()) {
            return true;
        }
        let name = self.identity.name().function_name();
        let parameter = request.and_then(|request| request.indirect.get(name));
        let matches = match (self.parameter.as_ref(), parameter) {
            (None, None) => true,
            (Some(left), Some(right)) => {
                left.scope == right.scope && left.value.bind(py).is(right.value.bind(py))
            }
            _ => false,
        };
        matches
            && self.dependencies.iter().all(|dependency| {
                dependency
                    .upgrade()
                    .is_some_and(|dependency| dependency.matches_parameters(py, request))
            })
    }

    #[expect(clippy::result_large_err)]
    fn value(&self, py: Python<'_>) -> Result<Py<PyAny>, FixtureCallError> {
        self.result
            .as_ref()
            .map(|value| value.clone_ref(py))
            .map_err(|error| error.clone_ref(py))
    }
}

impl FixtureRuntime {
    pub(super) fn cached_fixture(
        &self,
        py: Python<'_>,
        fixture: &NormalizedFixture,
        scope: ScopeKey<'_>,
    ) -> Option<Py<PyAny>> {
        let instance = self.cache.borrow().get(fixture.identity(), scope)?;
        instance
            .result
            .as_ref()
            .ok()
            .map(|value| value.clone_ref(py))
    }

    fn retire(&self, py: Python<'_>, fixture: &FixtureInstance) {
        if fixture.retired.replace(true) {
            return;
        }
        let dependents = fixture.dependents.take();
        for dependent in dependents
            .into_iter()
            .rev()
            .filter_map(|dependent| dependent.upgrade())
        {
            self.retire(py, &dependent);
        }
        let finalizer = fixture.finalizer.take();
        if let Some(finalizer) = finalizer {
            let diagnostics = finalizer.run(py);
            self.cleanup_diagnostics.borrow_mut().extend(diagnostics);
        }
        self.cache.borrow_mut().remove(fixture);
    }

    fn remember_dependency(&self, fixture: &Rc<FixtureInstance>) {
        if let Some(parent) = self.active.borrow_mut().last_mut()
            && !parent
                .dependencies
                .iter()
                .any(|dependency| Rc::ptr_eq(dependency, fixture))
        {
            parent.dependencies.push(Rc::clone(fixture));
        }
    }

    pub(super) fn override_origin(&self, name: &str) -> Option<FixtureIdentity> {
        self.active
            .borrow()
            .iter()
            .rev()
            .find(|frame| frame.identity.name().function_name() == name)
            .map(|frame| frame.identity.clone())
    }

    /// Prepares only dependencies at the current setup scope.
    #[expect(clippy::result_large_err)]
    fn prepare_dependencies_at_scope(
        &self,
        py: Python<'_>,
        plan: &FixturePlan,
        id: FixtureId,
        pass: &mut SetupPass<'_>,
        inputs: FixtureInputs<'_>,
    ) -> Result<(), FixtureCallError> {
        let request = inputs.request;
        let fixture = plan.fixture(id);
        let effective_scope = fixture_scope(fixture, request);
        match fixture_setup_order(plan, id, request, pass.setup_scopes)
            .scope
            .cmp(&pass.scope)
        {
            Ordering::Equal => return self.run_fixture(py, plan, id, inputs).map(drop),
            Ordering::Greater => return Ok(()),
            Ordering::Less => {}
        }
        if pass.visited.contains(&id)
            || (request.is_none_or(|request| request.indirect.is_empty())
                && fixture.broadest_scope < pass.scope)
        {
            return Ok(());
        }
        let key = scope_key(
            effective_scope,
            fixture.package_owner(),
            inputs.module_owner,
        );
        if self
            .cache
            .borrow()
            .get(fixture.identity(), key)
            .is_some_and(|cached| cached.matches_parameters(py, request.map(Rc::as_ref)))
        {
            return Ok(());
        }
        for dependency in fixture.dependencies() {
            if !effective_scope.can_use(dependency.scope(plan)) {
                // Full setup reports the mismatch before evaluating later arguments.
                break;
            }
            if let Some(id) = dependency.fixture_id() {
                self.prepare_dependencies_at_scope(py, plan, id, pass, inputs)
                    .map_err(|error| error.with_dependent(fixture))?;
                if !fixture_setup_order(plan, id, request, pass.setup_scopes).compatible {
                    break;
                }
            }
        }
        pass.visited.insert(id);
        Ok(())
    }

    /// Runs one fixture. Cache borrows end before entering user Python.
    #[expect(clippy::result_large_err)]
    pub(super) fn run_fixture(
        &self,
        py: Python<'_>,
        fixture_plan: &FixturePlan,
        fixture_id: FixtureId,
        inputs: FixtureInputs<'_>,
    ) -> Result<Py<PyAny>, FixtureCallError> {
        let request = inputs.request;
        let module_owner = inputs.module_owner;
        let fixture = fixture_plan.fixture(fixture_id);
        let effective_scope = fixture_scope(fixture, request);
        let scope = scope_key(effective_scope, fixture.package_owner(), module_owner);
        let parameter = request.and_then(|request| request.indirect.get(fixture.function_name()));
        if fixture.parameters.is_some() && parameter.is_none() {
            let message = format!(
                "fixture '{}' has no parameter defined for this test",
                fixture.function_name()
            );
            return Err(FixtureCallError::new(
                fixture,
                lookup_error(py, &message),
                FixtureArguments::default(),
            ));
        }
        let cached = self.cache.borrow().get(fixture.identity(), scope);
        if let Some(cached) = cached {
            if cached.matches_parameters(py, request.map(Rc::as_ref)) {
                self.remember_dependency(&cached);
                let value = cached.value(py)?;
                if let Some(request) = request {
                    request.record_fixture_name(fixture.function_name());
                }
                return Ok(value);
            }
            let was_tearing_down = self.tearing_down.replace(true);
            self.retire(py, &cached);
            self.tearing_down.set(was_tearing_down);
        }
        if request.is_some()
            && self
                .active
                .borrow()
                .iter()
                .any(|active| &active.identity == fixture.identity())
        {
            let message = format!(
                "recursive dependency involving fixture '{}'",
                fixture.function_name()
            );
            return Err(FixtureCallError::new(
                fixture,
                lookup_error(py, &message),
                FixtureArguments::default(),
            ));
        }

        self.active.borrow_mut().push(FixtureSetup {
            identity: fixture.identity().clone(),
            dependencies: Vec::new(),
        });
        let mut finalizer = None;
        let result = self.call_fixture(py, fixture_plan, fixture, inputs, &mut finalizer);
        let dependencies = self
            .active
            .borrow_mut()
            .pop()
            .map_or_else(Vec::new, |setup| setup.dependencies);
        if result.is_err() && finalizer.is_none() && parameter.is_none() {
            return result;
        }
        let instance = Rc::new(FixtureInstance {
            identity: fixture.identity().clone(),
            scope: effective_scope,
            package_owner: fixture.package_owner().to_path_buf(),
            module_owner: if effective_scope == FixtureScope::Module {
                module_owner.map(Utf8Path::to_path_buf)
            } else {
                None
            },
            parameter: parameter.cloned(),
            result: result.map_err(Box::new),
            has_parameters: parameter.is_some()
                || dependencies.iter().any(|item| item.has_parameters),
            finalizer: RefCell::new(finalizer),
            dependencies: dependencies.iter().map(Rc::downgrade).collect(),
            dependents: RefCell::default(),
            retired: Cell::new(false),
        });
        if effective_scope != FixtureScope::Function {
            for dependency in dependencies {
                let mut dependents = dependency.dependents.borrow_mut();
                dependents.retain(|dependent| dependent.strong_count() != 0);
                dependents.push(Rc::downgrade(&instance));
            }
        }
        self.cache.borrow_mut().insert(&instance);
        self.remember_dependency(&instance);
        let value = instance.value(py)?;
        if let Some(request) = request {
            request.record_fixture_name(fixture.function_name());
        }
        Ok(value)
    }

    #[expect(clippy::result_large_err)]
    fn call_fixture(
        &self,
        py: Python<'_>,
        fixture_plan: &FixturePlan,
        fixture: &NormalizedFixture,
        inputs: FixtureInputs<'_>,
        finalizer: &mut Option<Finalizer>,
    ) -> Result<Py<PyAny>, FixtureCallError> {
        let request = inputs.request;
        let mut arguments = Vec::new();
        let scope = fixture_scope(fixture, request);
        for dependency in fixture.dependencies() {
            let dependency_scope = dependency.scope(fixture_plan);
            let (kind, name) = match dependency {
                FixtureDependency::Fixture(id) => {
                    ("fixture", fixture_plan.fixture(*id).function_name())
                }
                FixtureDependency::Parameter(name) => ("parameter", name.as_ref()),
            };
            if !scope.can_use(dependency_scope) {
                let message = format!(
                    "ScopeMismatch: {}-scoped fixture '{}' cannot access {}-scoped {kind} '{name}'",
                    scope.name(),
                    fixture.function_name(),
                    dependency_scope.name(),
                );
                return Err(FixtureCallError::new(
                    fixture,
                    lookup_error(py, &message),
                    FixtureArguments::from_fixture_values(arguments),
                ));
            }
            let value = match dependency {
                FixtureDependency::Fixture(id) => self
                    .run_fixture(py, fixture_plan, *id, inputs)
                    .map_err(|error| error.with_dependent(fixture))?,
                FixtureDependency::Parameter(_) => {
                    let Some(value) = inputs.direct.and_then(|direct| direct.get(name)) else {
                        return Err(FixtureCallError::new(
                            fixture,
                            lookup_error(py, &format!("parameter '{name}' is unavailable")),
                            FixtureArguments::from_fixture_values(arguments),
                        ));
                    };
                    value.clone_ref(py)
                }
            };
            arguments.push((name, value));
        }

        let callbacks = fixture.takes_request().then(Callbacks::default);
        let result = (|| {
            if let (Some(request), Some(callbacks)) = (request, &callbacks) {
                let value = request.request(
                    py,
                    Some(fixture.identity().clone()),
                    request.fixture_scope(fixture),
                    fixture.package_owner(),
                    Rc::clone(callbacks),
                    request
                        .indirect
                        .get(fixture.function_name())
                        .map(|param| param.value.as_ref()),
                )?;
                if scope != FixtureScope::Function {
                    self.has_retained_requests.set(true);
                }
                arguments.push(("request", value));
            }
            let result = fixture.call(py, &arguments)?;
            get_value_and_generator(py, fixture, result)
        })();
        let (value, generator) = match result {
            Ok((value, generator)) => (Ok(value), generator),
            Err(error) => (Err(error), None),
        };
        if generator.is_some() || callbacks.is_some() {
            *finalizer = Some(Finalizer {
                generator,
                callbacks,
                is_async: fixture.statement().is_async,
                definition: Rc::clone(fixture.definition()),
            });
        }
        value.map_err(|error| {
            FixtureCallError::new(
                fixture,
                error,
                FixtureArguments::from_fixture_values(arguments),
            )
        })
    }

    /// Runs fixtures whose values are not passed to the test call.
    fn run_fixtures(
        &self,
        py: Python<'_>,
        fixture_plan: &FixturePlan,
        fixture_ids: &[FixtureId],
        usage: FixtureUsage,
        inputs: FixtureInputs<'_>,
    ) -> Vec<PreparedFixtureFailure> {
        fixture_ids
            .iter()
            .filter_map(|fixture_id| {
                let error = self
                    .run_fixture(py, fixture_plan, *fixture_id, inputs)
                    .err()?;
                Some(PreparedFixtureFailure::new(
                    fixture_plan.fixture(*fixture_id).function_name(),
                    usage,
                    error,
                ))
            })
            .collect()
    }
}

/// Case values and lifetime context shared by one fixture setup traversal.
#[derive(Clone, Copy, Default)]
pub(super) struct FixtureInputs<'a> {
    pub(super) module_owner: Option<&'a Utf8Path>,
    pub(super) request: Option<&'a Rc<RequestContext>>,
    pub(super) direct: Option<&'a HashMap<String, Arc<Py<PyAny>>>>,
}

fn request_setup_error(error: &PyErr) -> TestError {
    TestError::new(Diagnostic::new(
        "fixture-request",
        Severity::Error,
        format!("Failed to initialize fixture request: {error}"),
    ))
}

/// Shared traversal state for one pass through the fixture graph.
struct SetupPass<'a> {
    scope: FixtureScope,
    visited: HashSet<FixtureId>,
    setup_scopes: &'a mut HashMap<FixtureId, SetupOrder>,
}

/// Earliest setup pass and whether scope validation can reach later siblings.
#[derive(Clone, Copy)]
struct SetupOrder {
    scope: FixtureScope,
    compatible: bool,
}

fn fixture_setup_order(
    plan: &FixturePlan,
    id: FixtureId,
    request: Option<&Rc<RequestContext>>,
    orders: &mut HashMap<FixtureId, SetupOrder>,
) -> SetupOrder {
    let fixture = plan.fixture(id);
    let mut order = SetupOrder {
        scope: fixture_scope(fixture, request),
        compatible: true,
    };
    if request.is_none() || !fixture.needs_test_context() {
        return order;
    }
    if let Some(order) = orders.get(&id) {
        return *order;
    }

    let effective = order.scope;
    for dependency in fixture.dependencies() {
        if !effective.can_use(dependency.scope(plan)) {
            order.compatible = false;
            break;
        }
        if let Some(id) = dependency.fixture_id() {
            let dependency = fixture_setup_order(plan, id, request, orders);
            order.scope = order.scope.min(dependency.scope);
            if !dependency.compatible {
                order.compatible = false;
                break;
            }
        }
    }
    orders.insert(id, order);
    order
}

fn fixture_scope(
    fixture: &NormalizedFixture,
    request: Option<&Rc<RequestContext>>,
) -> FixtureScope {
    request.map_or_else(|| fixture.scope(), |request| request.fixture_scope(fixture))
}

pub(super) fn scope_key<'a>(
    scope: FixtureScope,
    package_owner: &'a Utf8Path,
    module_owner: Option<&'a Utf8Path>,
) -> ScopeKey<'a> {
    match scope {
        FixtureScope::Session => ScopeKey::Session,
        FixtureScope::Package => ScopeKey::Package(package_owner),
        FixtureScope::Module => ScopeKey::Module(module_owner.map_or(package_owner, |path| path)),
        FixtureScope::Function => ScopeKey::Function,
    }
}

/// Fixture state prepared for one test attempt.
pub(super) struct PreparedFixtures {
    /// Keyword arguments passed to the Python test function.
    pub(super) function_arguments: FixtureArguments,
    /// Whether fixture setup completed or blocked the test call.
    pub(super) setup_result: Result<(), TestError>,
    pub(super) skipped_setup: Option<TestExecutionOutcome>,
    pub(super) request: Option<Rc<RequestContext>>,
    pub(super) test_callbacks: Option<Callbacks>,
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
struct FixtureSetupError {
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

    /// Renders raw Python failures into one test-owned execution error.
    fn into_test_error(self, py: Python<'_>, verbose: bool) -> TestError {
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

    /// Converts a skip raised by fixture setup into the test's skipped outcome.
    fn skip_outcome(&self, py: Python<'_>) -> Option<TestExecutionOutcome> {
        let error = &self.first.error.error;
        is_skip_exception(py, error).then(|| TestExecutionOutcome::Skipped {
            reason: extract_skip_reason(py, error),
        })
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
#[derive(Clone)]
pub struct FixtureChainEntry {
    /// Fixture name shown in dependency diagnostics.
    pub(crate) name: String,
    /// Immutable fixture identity, syntax, and source.
    pub(crate) definition: Rc<FunctionDefinition>,
}

impl FixtureCallError {
    fn new(fixture: &NormalizedFixture, error: PyErr, arguments: FixtureArguments) -> Self {
        Self {
            fixture_name: fixture.function_name().to_string(),
            error,
            definition: Rc::clone(fixture.definition()),
            arguments,
            dependency_chain: Vec::new(),
        }
    }

    fn clone_ref(&self, py: Python<'_>) -> Self {
        Self {
            fixture_name: self.fixture_name.clone(),
            error: self.error.clone_ref(py),
            definition: Rc::clone(&self.definition),
            arguments: self.arguments.clone_ref(py),
            dependency_chain: self.dependency_chain.clone(),
        }
    }

    #[must_use]
    fn with_dependent(mut self, fixture: &NormalizedFixture) -> Self {
        self.dependency_chain.push(FixtureChainEntry {
            name: fixture.function_name().to_string(),
            definition: Rc::clone(fixture.definition()),
        });
        self
    }
}

/// Advances generator setup to its first yield, retaining the generator for teardown.
fn get_value_and_generator(
    py: Python<'_>,
    fixture: &NormalizedFixture,
    fixture_call_result: Py<PyAny>,
) -> PyResult<(Py<PyAny>, Option<Py<PyAny>>)> {
    if fixture.is_generator && fixture.statement().is_async {
        let anext_coroutine = fixture_call_result.bind(py).call_method0("__anext__")?;
        let value = run_coroutine(py, anext_coroutine.unbind())?;
        Ok((value, Some(fixture_call_result)))
    } else if fixture.is_generator {
        let mut iterator = fixture_call_result
            .bind(py)
            .cast::<PyIterator>()
            .map_err(|_| {
                pyo3::exceptions::PyTypeError::new_err(format!(
                    "Generator fixture `{}` did not return an iterator",
                    fixture.function_name(),
                ))
            })?
            .clone();
        let value = iterator.next().transpose()?.ok_or_else(|| {
            pyo3::exceptions::PyRuntimeError::new_err("Generator fixture yielded no value")
        })?;
        Ok((value.unbind(), Some(fixture_call_result)))
    } else {
        Ok((fixture_call_result, None))
    }
}
