//! Fixture setup, caching, and teardown for package-runner scopes.

use std::collections::HashMap;
use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;

use pyo3::prelude::*;
use pyo3::types::PyIterator;
use ruff_db::diagnostic::Diagnostic;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::diagnostic::{fixture_failure_diagnostic, fixture_resolution_diagnostic};
use crate::extensions::fixtures::{Finalizer, FixtureScope, HasFixtures, NormalizedFixture};
use crate::runner::FixtureArguments;
use crate::runner::fixture_resolver::RuntimeFixtureResolver;
use crate::utils::run_coroutine;

use super::PackageRunner;

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
        scope: FixtureScope,
    ) -> Result<(), Vec<Diagnostic>> {
        let mut resolver = RuntimeFixtureResolver::new(parents, current);
        let auto_use_fixtures = match resolver.get_normalized_auto_use_fixtures(py, scope) {
            Ok(fixtures) => fixtures,
            Err(error) => {
                let mut diagnostics = vec![fixture_resolution_diagnostic(error)];
                diagnostics.extend(self.clean_up_scope(py, scope));
                return Err(diagnostics);
            }
        };

        let auto_use_errors = self.run_fixtures(py, &auto_use_fixtures);
        if auto_use_errors.is_empty() {
            Ok(())
        } else {
            let mut diagnostics = auto_use_errors
                .into_iter()
                .map(|error| fixture_failure_diagnostic(py, error))
                .collect::<Vec<_>>();
            diagnostics.extend(self.clean_up_scope(py, scope));
            Err(diagnostics)
        }
    }

    /// Builds test-call arguments and runs every function-scoped fixture group.
    ///
    /// Returned finalizers belong directly to the test attempt. Broader-scoped
    /// finalizers are retained in the runner cache until their scope ends.
    pub(super) fn prepare_test_fixtures(
        &self,
        py: Python<'_>,
        fixture_dependencies: &[Rc<NormalizedFixture>],
        use_fixture_dependencies: &[Rc<NormalizedFixture>],
        auto_use_fixtures: &[Rc<NormalizedFixture>],
        params: HashMap<String, Arc<Py<PyAny>>>,
    ) -> PreparedFixtures {
        let mut test_finalizers = Vec::new();
        let mut fixture_call_errors = self.run_fixtures(py, use_fixture_dependencies);
        let mut function_arguments = FixtureArguments::default();

        for fixture in fixture_dependencies {
            match self.run_fixture(py, fixture) {
                Ok((value, finalizer)) => {
                    function_arguments
                        .insert(fixture.function_name().to_string(), value.clone_ref(py));

                    if let Some(finalizer) = finalizer {
                        test_finalizers.push(finalizer);
                    }
                }
                Err(error) => fixture_call_errors.push(error),
            }
        }

        fixture_call_errors.extend(self.run_fixtures(py, auto_use_fixtures));

        for (key, value) in params {
            function_arguments.insert(
                key,
                Arc::try_unwrap(value).unwrap_or_else(|arc| (*arc).clone_ref(py)),
            );
        }

        PreparedFixtures {
            function_arguments,
            fixture_call_errors,
            test_finalizers,
        }
    }

    /// Runs one fixture, recursively preparing dependencies first.
    #[expect(clippy::result_large_err)]
    fn run_fixture(
        &self,
        py: Python<'_>,
        fixture: &NormalizedFixture,
    ) -> Result<(Py<PyAny>, Option<Finalizer>), FixtureCallError> {
        if let Some(cached) = self
            .fixture_cache
            .get(py, fixture.function_name(), fixture.scope())
        {
            return Ok((cached, None));
        }

        let mut function_arguments = FixtureArguments::default();

        for dependency in fixture.dependencies() {
            match self.run_fixture(py, dependency) {
                Ok((value, finalizer)) => {
                    function_arguments
                        .insert(dependency.function_name().to_string(), value.clone_ref(py));

                    if let Some(finalizer) = finalizer {
                        self.finalizer_cache.add_finalizer(finalizer);
                    }
                }
                Err(mut error) => {
                    error.dependency_chain.push(FixtureChainEntry {
                        name: fixture.name.function_name().to_string(),
                        source_file: fixture.source_file.clone(),
                        stmt_function_def: Rc::clone(&fixture.stmt_function_def),
                    });
                    return Err(error);
                }
            }
        }

        let fixture_call_result =
            fixture
                .call(py, &function_arguments)
                .map_err(|error| FixtureCallError {
                    fixture_name: fixture.name.function_name().to_string(),
                    error,
                    stmt_function_def: Rc::clone(&fixture.stmt_function_def),
                    source_file: fixture.source_file.clone(),
                    arguments: function_arguments,
                    dependency_chain: Vec::new(),
                })?;

        let (value, finalizer) = get_value_and_finalizer(py, fixture, fixture_call_result)
            .map_err(|error| FixtureCallError {
                fixture_name: fixture.name.function_name().to_string(),
                error,
                stmt_function_def: Rc::clone(&fixture.stmt_function_def),
                source_file: fixture.source_file.clone(),
                arguments: FixtureArguments::default(),
                dependency_chain: Vec::new(),
            })?;

        self.fixture_cache.insert(
            fixture.function_name().to_string(),
            value.clone_ref(py),
            fixture.scope(),
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

    /// Clears cached fixture values and runs finalizers for one completed scope.
    pub(super) fn clean_up_scope(&self, py: Python<'_>, scope: FixtureScope) -> Vec<Diagnostic> {
        let diagnostics = self.finalizer_cache.run_and_clear_scope(py, scope);
        self.fixture_cache.clear_scope(scope);
        diagnostics
    }

    /// Cleans one scope and promotes teardown failures to run diagnostics.
    pub(super) fn report_scope_cleanup(&self, py: Python<'_>, scope: FixtureScope) {
        for diagnostic in self.clean_up_scope(py, scope) {
            self.context.add_run_diagnostic(diagnostic);
        }
    }

    /// Runs fixtures whose values are not passed to the test call.
    fn run_fixtures<P: Deref<Target = NormalizedFixture>>(
        &self,
        py: Python<'_>,
        fixtures: &[P],
    ) -> Vec<FixtureCallError> {
        let mut errors = Vec::new();
        for fixture in fixtures {
            match self.run_fixture(py, fixture) {
                Ok((_, Some(finalizer))) => self.finalizer_cache.add_finalizer(finalizer),
                Ok((_, None)) => {}
                Err(error) => errors.push(error),
            }
        }
        errors
    }
}

/// Fixture state prepared for one test attempt.
pub(super) struct PreparedFixtures {
    /// Keyword arguments passed to the Python test function.
    pub(super) function_arguments: FixtureArguments,
    /// Fixture setup failures that prevent the test call.
    pub(super) fixture_call_errors: Vec<FixtureCallError>,
    /// Function-scoped finalizers run after this attempt.
    pub(super) test_finalizers: Vec<Finalizer>,
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
            stmt_function_def: Some(Rc::clone(&fixture.stmt_function_def)),
            source_file: Some(fixture.source_file.clone()),
        };

        Ok((value, Some(finalizer)))
    } else if fixture.is_generator
        && let Ok(mut bound_iterator) = fixture_call_result
            .clone_ref(py)
            .into_bound(py)
            .cast_into::<PyIterator>()
    {
        match bound_iterator.next() {
            Some(Ok(value)) => {
                let finalizer = Finalizer {
                    fixture_return: bound_iterator.clone().unbind().into_any(),
                    is_async: false,
                    scope: fixture.scope(),
                    stmt_function_def: Some(Rc::clone(&fixture.stmt_function_def)),
                    source_file: Some(fixture.source_file.clone()),
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
