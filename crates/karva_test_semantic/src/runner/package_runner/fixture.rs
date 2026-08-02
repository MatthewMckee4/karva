//! Fixture setup, caching, and teardown for package-runner scopes.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use camino::Utf8Path;
use karva_diagnostic::{FixtureFailure, FixtureUsage};
use pyo3::prelude::*;
use pyo3::types::PyIterator;
use ruff_db::diagnostic::Diagnostic;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::diagnostic::fixture_resolution_diagnostic;
use crate::extensions::fixtures::{
    Finalizer, FixtureId, FixturePlan, FixtureScope, HasFixtures, NormalizedFixture,
};
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
        let scope_key = scope_key(scope, current_package);
        let mut compiler = FixturePlanCompiler::new(parents, current, current_package);
        let auto_use_fixtures = match compiler.get_normalized_auto_use_fixtures(py, scope) {
            Ok(fixtures) => fixtures,
            Err(error) => {
                return Err(TestError::new(fixture_resolution_diagnostic(error))
                    .with_related(self.clean_up_scope(py, scope_key)));
            }
        };
        let fixture_plan = compiler.finish();

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
    pub(super) fn clean_up_test_attempt(
        &mut self,
        py: Python<'_>,
        finalizers: Vec<Finalizer>,
    ) -> Vec<Diagnostic> {
        let mut diagnostics = finalizers
            .into_iter()
            .rev()
            .filter_map(|finalizer| finalizer.run(py).err())
            .collect::<Vec<_>>();
        diagnostics.extend(self.clean_up_scope(py, ScopeKey::Function));
        diagnostics
    }

    /// Clears cached fixture values and runs finalizers for one completed scope.
    pub(super) fn clean_up_scope(
        &mut self,
        py: Python<'_>,
        scope: ScopeKey<'_>,
    ) -> Vec<Diagnostic> {
        let diagnostics = self.finalizer_cache.run_and_clear_scope(py, scope);
        self.fixture_cache.clear_scope(scope);
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
    /// Returned finalizers belong directly to the test attempt. Broader-scoped
    /// finalizers are retained in the runner cache until their scope ends.
    pub(super) fn prepare_test_fixtures(
        &mut self,
        py: Python<'_>,
        fixture_plan: &FixturePlan,
        fixture_dependencies: &[FixtureId],
        use_fixture_dependencies: &[FixtureId],
        auto_use_fixtures: &[FixtureId],
        params: HashMap<String, Arc<Py<PyAny>>>,
    ) -> PreparedFixtures {
        let mut test_finalizers = Vec::new();
        let mut fixture_call_errors = self.run_fixtures(
            py,
            fixture_plan,
            use_fixture_dependencies,
            FixtureUsage::UseFixtures,
        );
        let mut function_arguments = FixtureArguments::default();

        for fixture_id in fixture_dependencies {
            let fixture = fixture_plan.fixture(*fixture_id);
            match self.run_fixture(py, fixture_plan, *fixture_id) {
                Ok((value, finalizer)) => {
                    function_arguments
                        .insert(fixture.function_name().to_string(), value.clone_ref(py));

                    if let Some(finalizer) = finalizer {
                        test_finalizers.push(finalizer);
                    }
                }
                Err(error) => fixture_call_errors.push(PreparedFixtureFailure::new(
                    fixture.function_name(),
                    FixtureUsage::Required,
                    error,
                )),
            }
        }

        fixture_call_errors.extend(self.run_fixtures(
            py,
            fixture_plan,
            auto_use_fixtures,
            FixtureUsage::AutoUse,
        ));

        for (key, value) in params {
            function_arguments.insert(
                key,
                Arc::try_unwrap(value).unwrap_or_else(|arc| (*arc).clone_ref(py)),
            );
        }

        PreparedFixtures {
            function_arguments,
            setup_result: FixtureSetupError::from_vec(fixture_call_errors).map_or(Ok(()), Err),
            test_finalizers,
        }
    }

    /// Runs one fixture, recursively preparing dependencies first.
    #[expect(clippy::result_large_err)]
    fn run_fixture(
        &mut self,
        py: Python<'_>,
        fixture_plan: &FixturePlan,
        fixture_id: FixtureId,
    ) -> Result<(Py<PyAny>, Option<Finalizer>), FixtureCallError> {
        let fixture = fixture_plan.fixture(fixture_id);
        let scope = scope_key(fixture.scope(), fixture.package_owner());
        if let Some(cached) = self.fixture_cache.get(py, fixture.function_name(), scope) {
            return Ok((cached, None));
        }

        let mut function_arguments = FixtureArguments::default();

        for dependency_id in fixture.dependencies() {
            let dependency = fixture_plan.fixture(*dependency_id);
            match self.run_fixture(py, fixture_plan, *dependency_id) {
                Ok((value, finalizer)) => {
                    function_arguments
                        .insert(dependency.function_name().to_string(), value.clone_ref(py));

                    if let Some(finalizer) = finalizer {
                        self.finalizer_cache.add_finalizer(finalizer);
                    }
                }
                Err(error) => return Err(error.with_dependent(fixture)),
            }
        }

        let fixture_call_result = match fixture.call(py, &function_arguments) {
            Ok(result) => result,
            Err(error) => return Err(FixtureCallError::new(fixture, error, function_arguments)),
        };

        let (value, finalizer) = get_value_and_finalizer(py, fixture, fixture_call_result)
            .map_err(|error| FixtureCallError::new(fixture, error, function_arguments))?;

        self.fixture_cache.insert(
            fixture.function_name().to_string(),
            value.clone_ref(py),
            scope,
        );

        let function_finalizer = finalizer.and_then(|finalizer| {
            if finalizer.scope == FixtureScope::Function {
                Some(finalizer)
            } else {
                self.finalizer_cache.add_finalizer(finalizer);
                None
            }
        });

        Ok((value, function_finalizer))
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
            match self.run_fixture(py, fixture_plan, *fixture_id) {
                Ok((_, Some(finalizer))) => self.finalizer_cache.add_finalizer(finalizer),
                Ok((_, None)) => {}
                Err(error) => errors.push(PreparedFixtureFailure::new(
                    fixture.function_name(),
                    usage,
                    error,
                )),
            }
        }
        errors
    }
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
    /// Function-scoped finalizers run after this attempt.
    pub(super) test_finalizers: Vec<Finalizer>,
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
    /// Fixture definition used to locate the failure.
    pub(crate) stmt_function_def: Rc<StmtFunctionDef>,
    /// Source containing the failing fixture.
    pub(crate) source_file: SourceFile,
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
    /// Source containing this intermediate fixture.
    pub(crate) source_file: SourceFile,
    /// Fixture definition used to locate this dependency step.
    pub(crate) stmt_function_def: Rc<StmtFunctionDef>,
}

impl FixtureCallError {
    fn new(fixture: &NormalizedFixture, error: PyErr, arguments: FixtureArguments) -> Self {
        Self {
            fixture_name: fixture.function_name().to_string(),
            error,
            stmt_function_def: Rc::clone(&fixture.stmt_function_def),
            source_file: fixture.source_file.clone(),
            arguments,
            dependency_chain: Vec::new(),
        }
    }

    #[must_use]
    fn with_dependent(mut self, fixture: &NormalizedFixture) -> Self {
        self.dependency_chain.push(FixtureChainEntry {
            name: fixture.function_name().to_string(),
            source_file: fixture.source_file.clone(),
            stmt_function_def: Rc::clone(&fixture.stmt_function_def),
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
    if fixture.is_generator && fixture.stmt_function_def.is_async {
        let bound = fixture_call_result.bind(py);
        let anext_coroutine = bound.call_method0("__anext__")?;
        let value = run_coroutine(py, anext_coroutine.unbind())?;

        let finalizer = Finalizer {
            fixture_return: fixture_call_result,
            is_async: true,
            scope: fixture.scope(),
            package_owner: fixture.package_owner().to_path_buf(),
            stmt_function_def: Rc::clone(&fixture.stmt_function_def),
            source_file: fixture.source_file.clone(),
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
                    scope: fixture.scope(),
                    package_owner: fixture.package_owner().to_path_buf(),
                    stmt_function_def: Rc::clone(&fixture.stmt_function_def),
                    source_file: fixture.source_file.clone(),
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
