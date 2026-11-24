//! Fixture setup, caching, and teardown for package-runner scopes.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use camino::Utf8Path;
use karva_diagnostic::{Diagnostic, FixtureFailure, FixtureUsage, TestExecutionOutcome};
use pyo3::prelude::*;
use pyo3::types::PyIterator;

use crate::diagnostic::fixture_resolution_diagnostic;
use crate::discovery::models::definition::FunctionDefinition;
use crate::extensions::fixtures::{
    Finalizer, FixtureId, FixturePlan, FixtureScope, HasFixtures, NormalizedFixture,
};
use crate::extensions::tags::skip::{extract_skip_reason, is_skip_exception};
use crate::runner::FixtureArguments;
use crate::runner::fixture_resolver::FixturePlanCompiler;
use crate::runner::scoped_storage::ScopeKey;
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
        &mut self,
        py: Python<'_>,
        parents: &'a [&'a crate::discovery::DiscoveredPackage],
        current: &'a (dyn HasFixtures<'a> + 'a),
        current_package: &'a Utf8Path,
        scope: FixtureScope,
    ) -> Result<(), TestError> {
        let module_owner = self.active_module.clone();
        let scope_key = scope_key(scope, current_package, module_owner.as_deref());
        let mut compiler = FixturePlanCompiler::new(parents, current, current_package);
        let auto_use_fixtures = match compiler.get_normalized_auto_use_fixtures(py, scope) {
            Ok(fixtures) => fixtures,
            Err(error) => {
                return Err(TestError::new(fixture_resolution_diagnostic(error))
                    .with_related(self.clean_up_scope(py, scope_key)));
            }
        };
        let fixture_plan = compiler.finish();

        // Commit the preceding result before a broader-scope fixture can
        // terminate the worker. Empty scopes leave delivery to the next test
        // checkpoint, which combines both frames in one socket flush.
        if !auto_use_fixtures.is_empty() {
            self.context.flush_test_results();
        }
        let failures =
            self.run_fixtures(py, &fixture_plan, &auto_use_fixtures, FixtureUsage::AutoUse);
        let Some(failures) = FixtureSetupError::from_vec(failures) else {
            return Ok(());
        };

        Err(failures
            .into_test_error(py, self.context.is_verbose())
            .with_related(self.clean_up_scope(py, scope_key)))
    }

    /// Runs function-scoped finalizers and clears their cached fixture values.
    pub(super) fn clean_up_test_attempt(&mut self, py: Python<'_>) -> Vec<Diagnostic> {
        self.clean_up_scope(py, ScopeKey::Function)
    }

    /// Clears cached fixture values and runs finalizers for one completed scope.
    fn clean_up_scope(&mut self, py: Python<'_>, scope: ScopeKey<'_>) -> Vec<Diagnostic> {
        let diagnostics = self.finalizer_cache.run_and_clear_scope(py, scope);
        self.fixture_cache.clear_scope(scope);
        diagnostics
    }

    /// Cleans one scope and promotes teardown failures to run diagnostics.
    ///
    /// Results need committing only when cleanup can execute user Python:
    /// finalizers run directly, while releasing a cached value may invoke
    /// `__del__`. Empty cleanup stays buffered until the next checkpoint.
    pub(super) fn report_scope_cleanup(&mut self, py: Python<'_>, scope: ScopeKey<'_>) {
        if self.finalizer_cache.has_finalizers(scope) || self.fixture_cache.has_values(scope) {
            self.context.flush_test_results();
        }
        for diagnostic in self.clean_up_scope(py, scope) {
            self.state.add_run_diagnostic(diagnostic);
        }
    }

    /// Gives auto-use requests priority without moving them ahead of broader dependencies.
    pub(super) fn prepare_test_fixtures(
        &mut self,
        py: Python<'_>,
        fixture_plan: &FixturePlan,
        fixture_dependencies: &[FixtureId],
        use_fixture_dependencies: &[FixtureId],
        auto_use_fixtures: &[FixtureId],
        params: &HashMap<String, Arc<Py<PyAny>>>,
    ) -> PreparedFixtures {
        let mut requests: Vec<_> = auto_use_fixtures
            .iter()
            .map(|&id| (id, FixtureUsage::AutoUse))
            .chain(
                use_fixture_dependencies
                    .iter()
                    .map(|&id| (id, FixtureUsage::UseFixtures)),
            )
            .chain(
                fixture_dependencies
                    .iter()
                    .map(|&id| (id, FixtureUsage::Required)),
            )
            .collect();
        let mut fixture_call_errors = Vec::new();
        let mut function_arguments = FixtureArguments::default();

        // Scoped autouse fixtures also appear in the request list, even when
        // there is no function-scoped autouse fixture to order against.
        let has_function_auto_use = auto_use_fixtures
            .iter()
            .any(|&id| fixture_plan.fixture(id).scope() == FixtureScope::Function);
        let setup_scopes = if has_function_auto_use {
            FixtureScope::Function.scopes_above()
        } else {
            &[FixtureScope::Function]
        };
        for &scope in setup_scopes.iter().rev() {
            let mut visited = HashSet::new();
            requests.retain(|&(id, usage)| {
                let fixture = fixture_plan.fixture(id);
                let run_request = scope == FixtureScope::Function || fixture.scope() == scope;
                let result = if run_request {
                    self.run_fixture(py, fixture_plan, id).map(|value| {
                        if usage == FixtureUsage::Required {
                            function_arguments.insert(fixture.function_name().to_string(), value);
                        }
                    })
                } else {
                    self.prepare_dependencies_at_scope(py, fixture_plan, id, scope, &mut visited)
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

        for (key, value) in params {
            function_arguments.insert(key.clone(), value.clone_ref(py));
        }

        PreparedFixtures {
            function_arguments,
            setup_result: FixtureSetupError::from_vec(fixture_call_errors).map_or(Ok(()), Err),
        }
    }

    /// Prepares only the dependencies in the current scope of a narrower request.
    #[expect(clippy::result_large_err)]
    fn prepare_dependencies_at_scope(
        &mut self,
        py: Python<'_>,
        fixture_plan: &FixturePlan,
        id: FixtureId,
        scope: FixtureScope,
        visited: &mut HashSet<FixtureId>,
    ) -> Result<(), FixtureCallError> {
        let fixture = fixture_plan.fixture(id);
        match fixture.scope().cmp(&scope) {
            Ordering::Equal => return self.run_fixture(py, fixture_plan, id).map(drop),
            Ordering::Greater => return Ok(()),
            Ordering::Less => {}
        }
        let cache_scope = scope_key(
            fixture.scope(),
            fixture.package_owner(),
            self.active_module.as_deref(),
        );
        if fixture.broadest_scope < scope
            || visited.contains(&id)
            || self.fixture_cache.contains(fixture.identity(), cache_scope)
        {
            return Ok(());
        }
        for &dependency in fixture.dependencies() {
            self.prepare_dependencies_at_scope(py, fixture_plan, dependency, scope, visited)
                .map_err(|error| error.with_dependent(fixture))?;
        }
        visited.insert(id);
        Ok(())
    }

    /// Runs a fixture after its dependencies and registers teardown in setup order.
    #[expect(clippy::result_large_err)]
    fn run_fixture(
        &mut self,
        py: Python<'_>,
        fixture_plan: &FixturePlan,
        fixture_id: FixtureId,
    ) -> Result<Py<PyAny>, FixtureCallError> {
        let fixture = fixture_plan.fixture(fixture_id);
        let module_owner = self.active_module.clone();
        let scope = scope_key(
            fixture.scope(),
            fixture.package_owner(),
            module_owner.as_deref(),
        );
        if let Some(cached) = self.fixture_cache.get(py, fixture.identity(), scope) {
            return Ok(cached);
        }

        let mut function_arguments = Vec::new();

        for dependency_id in fixture.dependencies() {
            let dependency = fixture_plan.fixture(*dependency_id);
            match self.run_fixture(py, fixture_plan, *dependency_id) {
                Ok(value) => {
                    if function_arguments.is_empty() {
                        // Delay allocation until recursion unwinds so deep chains do not
                        // retain empty argument buffers for every active fixture call.
                        function_arguments.reserve_exact(fixture.dependencies().len());
                    }
                    function_arguments.push((dependency.function_name(), value));
                }
                Err(error) => return Err(error.with_dependent(fixture)),
            }
        }

        let fixture_call_result = match fixture.call(py, &function_arguments) {
            Ok(result) => result,
            Err(error) => {
                return Err(FixtureCallError::new(
                    fixture,
                    error,
                    FixtureArguments::from_fixture_values(function_arguments),
                ));
            }
        };

        let (value, finalizer) = get_value_and_finalizer(py, fixture, fixture_call_result)
            .map_err(|error| {
                FixtureCallError::new(
                    fixture,
                    error,
                    FixtureArguments::from_fixture_values(function_arguments),
                )
            })?;

        self.fixture_cache
            .insert(fixture.identity().clone(), value.clone_ref(py), scope);

        if let Some(finalizer) = finalizer {
            self.finalizer_cache.add_finalizer(scope, finalizer);
        }

        Ok(value)
    }

    /// Runs fixtures whose values are not passed to the test call.
    fn run_fixtures(
        &mut self,
        py: Python<'_>,
        fixture_plan: &FixturePlan,
        fixture_ids: &[FixtureId],
        usage: FixtureUsage,
    ) -> Vec<PreparedFixtureFailure> {
        let mut errors = Vec::new();
        for fixture_id in fixture_ids {
            let fixture = fixture_plan.fixture(*fixture_id);
            if let Err(error) = self.run_fixture(py, fixture_plan, *fixture_id) {
                errors.push(PreparedFixtureFailure::new(
                    fixture.function_name(),
                    usage,
                    error,
                ));
            }
        }
        errors
    }
}

fn scope_key<'a>(
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

    /// Converts a skip raised by fixture setup into the test's skipped outcome.
    pub(super) fn skip_outcome(&self, py: Python<'_>) -> Option<TestExecutionOutcome> {
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

    #[must_use]
    fn with_dependent(mut self, fixture: &NormalizedFixture) -> Self {
        self.dependency_chain.push(FixtureChainEntry {
            name: fixture.function_name().to_string(),
            definition: Rc::clone(fixture.definition()),
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

        let finalizer = Finalizer {
            fixture_return: fixture_call_result,
            is_async: true,
            definition: Rc::clone(fixture.definition()),
        };

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
                let finalizer = Finalizer {
                    fixture_return: bound_iterator.clone().unbind().into_any(),
                    is_async: false,
                    definition: Rc::clone(fixture.definition()),
                };

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
