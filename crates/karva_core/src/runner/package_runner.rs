use std::collections::HashMap;

use pyo3::{
    prelude::*,
    types::{PyDict, PyIterator},
};

use crate::{
    Context, IndividualTestResultKind,
    diagnostic::{Diagnostic, FunctionDefinitionLocation, FunctionKind},
    extensions::{
        fixtures::{
            Finalizer, FixtureScope, NormalizedFixture, NormalizedFixtureValue, RequiresFixtures,
            missing_arguments_from_error, python::FixtureRequest,
        },
        tags::{ExpectFailTag, python::SkipError},
    },
    normalize::models::{NormalizedModule, NormalizedPackage, NormalizedTestFunction},
    runner::{FinalizerCache, FixtureCache},
    utils::full_test_name,
};

pub struct NormalizedPackageRunner<'ctx, 'proj, 'rep> {
    context: &'ctx mut Context<'proj, 'rep>,
    fixture_cache: FixtureCache,
    finalizer_cache: FinalizerCache,
}

impl<'ctx, 'proj, 'rep> NormalizedPackageRunner<'ctx, 'proj, 'rep> {
    pub(crate) fn new(context: &'ctx mut Context<'proj, 'rep>) -> Self {
        Self {
            context,
            fixture_cache: FixtureCache::default(),
            finalizer_cache: FinalizerCache::default(),
        }
    }

    fn clean_up(&mut self, py: Python, scope: FixtureScope) {
        let diagnostics = self.finalizer_cache.run_and_clear_scope(py, scope);

        self.context.result_mut().add_test_diagnostics(diagnostics);
        self.fixture_cache.clear_fixtures(scope);
    }

    pub(crate) fn run(&mut self, py: Python<'_>, session: &NormalizedPackage) {
        for fixture in &session.auto_use_fixtures {
            if let Ok((_, Some(finalizer))) = self.execute_normalized_fixture(py, fixture) {
                self.finalizer_cache.add_finalizer(finalizer);
            }
        }

        self.run_package(py, session);

        self.clean_up(py, FixtureScope::Session);
    }

    fn run_module(&mut self, py: Python<'_>, module: &NormalizedModule) -> bool {
        for fixture in &module.auto_use_fixtures {
            if let Ok((_, Some(finalizer))) = self.execute_normalized_fixture(py, fixture) {
                self.finalizer_cache.add_finalizer(finalizer);
            }
        }

        let mut passed = true;

        for test_function in &module.test_functions {
            let test_passed = self.run_normalized_test(py, test_function);
            passed &= test_passed;

            if self.context.project().options().fail_fast() && !passed {
                break;
            }
        }

        self.clean_up(py, FixtureScope::Module);

        passed
    }

    fn run_package(&mut self, py: Python<'_>, package: &NormalizedPackage) -> bool {
        for fixture in &package.auto_use_fixtures {
            if let Ok((_, Some(finalizer))) = self.execute_normalized_fixture(py, fixture) {
                self.finalizer_cache.add_finalizer(finalizer);
            }
        }

        let mut passed = true;

        for module in package.modules.values() {
            passed &= self.run_module(py, module);

            if self.context.project().options().fail_fast() && !passed {
                break;
            }
        }

        for sub_package in package.packages.values() {
            passed &= self.run_package(py, sub_package);

            if self.context.project().options().fail_fast() && !passed {
                break;
            }
        }

        self.clean_up(py, FixtureScope::Package);

        passed
    }

    /// Run a normalized test function.
    fn run_normalized_test(&mut self, py: Python<'_>, test_fn: &NormalizedTestFunction) -> bool {
        // Check if test should be skipped
        if let Some(skip_tag) = test_fn.tags().skip_tag() {
            if skip_tag.should_skip() {
                let reporter = self.context.reporter();

                self.context.result_mut().register_test_case_result(
                    &test_fn.name().to_string(),
                    IndividualTestResultKind::Skipped {
                        reason: skip_tag.reason(),
                    },
                    Some(reporter),
                );
                return true;
            }
        }

        // Check if test is expected to fail
        let expect_fail_tag = test_fn.tags().expect_fail_tag();
        let expect_fail = expect_fail_tag
            .as_ref()
            .is_some_and(ExpectFailTag::should_expect_fail);

        let mut test_finalizers = Vec::new();

        let cwd = self.context.project().cwd().clone();

        let handle_fixture_fail = |fixture: &NormalizedFixture, err: PyErr| {
            let default_diagnostic = || {
                Diagnostic::from_fixture_fail(
                    py,
                    &cwd,
                    &err,
                    FunctionDefinitionLocation::new(
                        fixture.name().to_string(),
                        fixture.location.clone(),
                    ),
                )
            };

            let missing_args =
                missing_arguments_from_error(fixture.name().function_name(), &err.to_string());

            if missing_args.is_empty() {
                default_diagnostic()
            } else {
                Diagnostic::missing_fixtures(
                    missing_args,
                    fixture.location.clone(),
                    fixture.name().to_string(),
                    FunctionKind::Fixture,
                )
            }
        };

        // Execute use_fixtures (for side effects only, don't pass to test)
        for fixture in test_fn.use_fixture_dependencies() {
            match self.execute_normalized_fixture(py, fixture) {
                Ok((_, finalizer)) => {
                    if let Some(f) = finalizer {
                        test_finalizers.push(f);
                    }
                }
                Err(err) => {
                    let diagnostic = handle_fixture_fail(fixture, err);
                    self.context.result_mut().add_test_diagnostic(diagnostic);
                }
            }
        }

        // Execute regular fixture dependencies and add them to kwargs
        let mut kwargs = HashMap::new();
        for fixture in test_fn.fixture_dependencies() {
            match self.execute_normalized_fixture(py, fixture) {
                Ok((value, finalizer)) => {
                    kwargs.insert(fixture.name().function_name(), value);
                    if let Some(f) = finalizer {
                        test_finalizers.push(f);
                    }
                }
                Err(err) => {
                    let diagnostic = handle_fixture_fail(fixture, err);
                    self.context.result_mut().add_test_diagnostic(diagnostic);
                }
            }
        }

        // Add test params to kwargs
        for (key, value) in test_fn.params() {
            kwargs.insert(key, value.clone());
        }

        let full_test_name = full_test_name(py, test_fn.name().to_string(), &kwargs);

        for fixture in &test_fn.auto_use_fixtures {
            if let Ok((_, Some(finalizer))) = self.execute_normalized_fixture(py, fixture) {
                self.finalizer_cache.add_finalizer(finalizer);
            }
        }

        // Call the test function
        let test_result = if kwargs.is_empty() {
            test_fn.function().call0(py)
        } else {
            let py_dict = PyDict::new(py);
            for (key, value) in &kwargs {
                let _ = py_dict.set_item(key, value);
            }
            test_fn.function().call(py, (), Some(&py_dict))
        };

        let reporter = self.context.reporter();
        let passed = match test_result {
            Ok(_) => {
                if expect_fail {
                    let reason = expect_fail_tag.and_then(|tag| tag.reason());

                    let diagnostic = Diagnostic::pass_on_expect_fail(
                        reason,
                        FunctionDefinitionLocation::new(
                            full_test_name.clone(),
                            Some(test_fn.location.clone()),
                        ),
                    );

                    let result = self.context.result_mut();

                    result.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Failed,
                        Some(reporter),
                    );

                    result.add_test_diagnostic(diagnostic);

                    false
                } else {
                    self.context.result_mut().register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Passed,
                        Some(reporter),
                    );

                    true
                }
            }
            Err(err) => {
                if is_skip_exception(py, &err) {
                    let reason = extract_skip_reason(py, &err);
                    self.context.result_mut().register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Skipped { reason },
                        Some(reporter),
                    );
                    true
                } else if expect_fail {
                    self.context.result_mut().register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Passed,
                        Some(reporter),
                    );
                    true
                } else {
                    let default_diagnostic = || {
                        Diagnostic::from_test_fail(
                            py,
                            &cwd,
                            &err,
                            FunctionDefinitionLocation::new(
                                full_test_name.clone(),
                                Some(test_fn.location.clone()),
                            ),
                        )
                    };

                    let missing_args = missing_arguments_from_error(
                        test_fn.name().function_name(),
                        &err.to_string(),
                    );

                    let diagnostic = if missing_args.is_empty() {
                        default_diagnostic()
                    } else {
                        Diagnostic::missing_fixtures(
                            missing_args,
                            Some(test_fn.location.clone()),
                            full_test_name.clone(),
                            FunctionKind::Test,
                        )
                    };

                    self.context.result_mut().add_test_diagnostic(diagnostic);

                    self.context.result_mut().register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Failed,
                        Some(reporter),
                    );

                    false
                }
            }
        };

        // Run function-scoped finalizers for this test in reverse order (LIFO)
        for finalizer in test_finalizers.into_iter().rev() {
            if let Some(diagnostic) = finalizer.run(py) {
                self.context.result_mut().add_test_diagnostic(diagnostic);
            }
        }

        self.clean_up(py, FixtureScope::Function);

        passed
    }

    /// Execute a normalized fixture and all its dependencies, returning the fixture value and optional finalizer.
    fn execute_normalized_fixture(
        &mut self,
        py: Python<'_>,
        fixture: &NormalizedFixture,
    ) -> PyResult<(Py<PyAny>, Option<Finalizer>)> {
        if let Some(cached) = self
            .fixture_cache
            .get(fixture.name().function_name(), fixture.scope)
        {
            // Cached fixtures have already had their finalizers stored/run
            return Ok((cached.clone(), None));
        }

        // Execute dependencies first
        let mut dep_kwargs = HashMap::new();
        for dep in fixture.dependencies() {
            let (dep_value, dep_finalizer) = self.execute_normalized_fixture(py, dep)?;

            // Dependency finalizers are always added to the cache
            // They need to outlive the current fixture execution
            if let Some(finalizer) = dep_finalizer {
                self.finalizer_cache.add_finalizer(finalizer);
            }

            dep_kwargs.insert(dep.name().function_name(), dep_value);
        }

        // Build kwargs for this fixture
        let kwargs_dict = PyDict::new(py);
        for (key, value) in &dep_kwargs {
            kwargs_dict.set_item(key, value)?;
        }

        // For builtin fixtures, the value is stored directly in the function field
        // and function_definition is None. Return the value directly without calling.
        let result = match &fixture.value {
            NormalizedFixtureValue::Computed(value) => value.clone(),

            NormalizedFixtureValue::Function(function) => {
                if let Some(function_def) = fixture.function_definition() {
                    let required = function_def.required_fixtures(py);

                    if required.contains(&"request".to_string()) {
                        // Create FixtureRequest with param (or None if not parametrized)
                        let param_value = fixture.param().map_or_else(|| py.None(), Clone::clone);

                        let request_obj = Py::new(py, FixtureRequest::new(param_value))?;
                        kwargs_dict.set_item("request", request_obj)?;
                    }
                }

                if kwargs_dict.is_empty() {
                    function.call0(py)?
                } else {
                    function.call(py, (), Some(&kwargs_dict))?
                }
            }
        };

        // If this is a generator fixture, we need to call next() to get the actual value
        // and create a finalizer for cleanup
        let (final_result, finalizer) = if fixture.is_generator {
            let bound_result = result.bind(py);
            if let Ok(py_iter_bound) = bound_result.cast::<PyIterator>() {
                let py_iter: Py<PyIterator> = py_iter_bound.clone().unbind();
                let mut iterator = py_iter.bind(py).clone();
                match iterator.next() {
                    Some(Ok(value)) => {
                        let finalizer =
                            Finalizer::new(fixture.name().to_string(), py_iter, fixture.scope);

                        (value.unbind(), Some(finalizer))
                    }
                    Some(Err(err)) => return Err(err),
                    None => (result, None),
                }
            } else {
                (result, None)
            }
        } else {
            (result, None)
        };

        // Cache the result
        self.fixture_cache.insert(
            fixture.name().function_name().to_string(),
            final_result.clone(),
            fixture.scope,
        );

        // Handle finalizer based on scope
        // Function-scoped finalizers are returned to be run immediately after the test
        // Higher-scoped finalizers are added to the cache
        let return_finalizer = finalizer.map_or_else(
            || None,
            |f| {
                if f.scope() == FixtureScope::Function {
                    Some(f)
                } else {
                    self.finalizer_cache.add_finalizer(f);
                    None
                }
            },
        );

        Ok((final_result, return_finalizer))
    }
}

/// Check if the given `PyErr` is a skip exception.
fn is_skip_exception(py: Python<'_>, err: &PyErr) -> bool {
    // Check for karva.SkipError
    if err.is_instance_of::<SkipError>(py) {
        return true;
    }

    // Check for pytest skip exception
    if let Ok(pytest_module) = py.import("_pytest.outcomes")
        && let Ok(skipped) = pytest_module.getattr("Skipped")
        && err.matches(py, skipped).unwrap_or(false)
    {
        return true;
    }

    false
}

/// Extract the skip reason from a skip exception.
fn extract_skip_reason(py: Python<'_>, err: &PyErr) -> Option<String> {
    let value = err.value(py);

    // Try to get the first argument (the message)
    if let Ok(args) = value.getattr("args")
        && let Ok(tuple) = args.cast::<pyo3::types::PyTuple>()
        && let Ok(first_arg) = tuple.get_item(0)
        && let Ok(message) = first_arg.extract::<String>()
    {
        if message.is_empty() {
            return None;
        }
        return Some(message);
    }

    None
}
