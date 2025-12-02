use std::collections::HashMap;

use pyo3::{
    prelude::*,
    types::{PyDict, PyIterator},
};

use crate::{
    Context, FunctionKind, IndividualTestResultKind,
    diagnostic::{
        report_fixture_failure, report_missing_fixtures, report_test_failure,
        report_test_pass_on_expect_failure,
    },
    extensions::{
        fixtures::{
            Finalizer, FixtureRequest, FixtureScope, NormalizedFixture, NormalizedFixtureValue,
            RequiresFixtures, missing_arguments_from_error,
        },
        tags::{
            expect_fail::ExpectFailTag,
            skip::{extract_skip_reason, is_skip_exception},
        },
    },
    normalize::{NormalizedModule, NormalizedPackage, NormalizedTest},
    runner::{FinalizerCache, FixtureCache},
    utils::{full_test_name, source_file},
};

/// A struct that is used to execute tests within a package.
///
/// We assume a normalized state of the package.
pub struct NormalizedPackageRunner<'ctx, 'proj, 'rep> {
    context: &'ctx Context<'proj, 'rep>,
    fixture_cache: FixtureCache,
    finalizer_cache: FinalizerCache,
}

impl<'ctx, 'proj, 'rep> NormalizedPackageRunner<'ctx, 'proj, 'rep> {
    pub(crate) fn new(context: &'ctx Context<'proj, 'rep>) -> Self {
        Self {
            context,
            fixture_cache: FixtureCache::default(),
            finalizer_cache: FinalizerCache::default(),
        }
    }

    /// Executes all tests in a package.
    ///
    /// The main entrypoint for actual test execution.
    pub(crate) fn execute(&self, py: Python<'_>, session: NormalizedPackage) {
        self.execute_fixtures(py, &session.auto_use_fixtures);

        self.execute_package(py, session);

        self.clean_up_scope(py, FixtureScope::Session);
    }

    /// Execute a module.
    ///
    /// Executes all tests in a module.
    ///
    /// Failing fast if the user has specified that we should.
    fn execute_module(&self, py: Python<'_>, module: NormalizedModule) -> bool {
        self.execute_fixtures(py, &module.auto_use_fixtures);

        let mut passed = true;

        for test_function in module.test_functions {
            passed &= self.execute_test(py, test_function);

            if self.context.project().options().fail_fast() && !passed {
                break;
            }
        }

        self.clean_up_scope(py, FixtureScope::Module);

        passed
    }

    /// Execute a package.
    ///
    /// Executes all tests in each module and sub-package.
    ///
    /// Failing fast if the user has specified that we should.
    fn execute_package(&self, py: Python<'_>, package: NormalizedPackage) -> bool {
        let NormalizedPackage {
            modules,
            packages,
            auto_use_fixtures,
        } = package;

        self.execute_fixtures(py, &auto_use_fixtures);

        let mut passed = true;

        for module in modules.into_values() {
            passed &= self.execute_module(py, module);

            if self.context.project().options().fail_fast() && !passed {
                break;
            }
        }

        if !self.context.project().options().fail_fast() || passed {
            for sub_package in packages.into_values() {
                passed &= self.execute_package(py, sub_package);

                if self.context.project().options().fail_fast() && !passed {
                    break;
                }
            }
        }

        self.clean_up_scope(py, FixtureScope::Package);

        passed
    }

    /// Run a normalized test function.
    fn execute_test(&self, py: Python<'_>, test: NormalizedTest) -> bool {
        let tags = test.resolved_tags();
        let test_module_path = test.module_path().clone();

        let NormalizedTest {
            name,
            params,
            fixture_dependencies,
            use_fixture_dependencies,
            auto_use_fixtures,
            function,
            tags: _,
            stmt_function_def,
        } = test;

        if let (true, reason) = tags.should_skip() {
            return self.context.register_test_case_result(
                &name.to_string(),
                IndividualTestResultKind::Skipped { reason },
            );
        }

        let expect_fail_tag = tags.expect_fail_tag();
        let expect_fail = expect_fail_tag
            .as_ref()
            .is_some_and(ExpectFailTag::should_expect_fail);

        let mut test_finalizers = Vec::new();

        self.execute_fixtures(py, &use_fixture_dependencies);

        let mut test_arguments = HashMap::new();

        for fixture in fixture_dependencies.iter() {
            if let Some((value, finalizer)) = self.execute_fixture(py, fixture) {
                test_arguments.insert(fixture.function_name().to_string(), value);
                if let Some(finalizer) = finalizer {
                    test_finalizers.push(finalizer);
                }
            }
        }

        self.execute_fixtures(py, &auto_use_fixtures);

        for (key, value) in params {
            test_arguments.insert(key, value);
        }

        let full_test_name = full_test_name(py, name.to_string(), &test_arguments);

        tracing::info!("Running test `{}`", full_test_name);

        let test_result = if test_arguments.is_empty() {
            function.call0(py)
        } else {
            let py_dict = PyDict::new(py);
            for (key, value) in &test_arguments {
                let _ = py_dict.set_item(key, value);
            }
            function.call(py, (), Some(&py_dict))
        };

        let passed = match test_result {
            Ok(_) => {
                if expect_fail {
                    let reason = expect_fail_tag.and_then(|tag| tag.reason());

                    report_test_pass_on_expect_failure(
                        self.context,
                        source_file(&test_module_path),
                        &stmt_function_def,
                        reason,
                    );

                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Failed,
                    )
                } else {
                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Passed,
                    )
                }
            }
            Err(err) => {
                if is_skip_exception(py, &err) {
                    let reason = extract_skip_reason(py, &err);
                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Skipped { reason },
                    )
                } else if expect_fail {
                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Passed,
                    )
                } else {
                    let missing_args =
                        missing_arguments_from_error(name.function_name(), &err.to_string());

                    if missing_args.is_empty() {
                        report_test_failure(
                            self.context,
                            py,
                            source_file(&test_module_path),
                            &stmt_function_def,
                            &test_arguments,
                            &err,
                        );
                    } else {
                        report_missing_fixtures(
                            self.context,
                            source_file(&test_module_path),
                            &stmt_function_def,
                            &missing_args,
                            FunctionKind::Test,
                        );
                    }

                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Failed,
                    )
                }
            }
        };

        for finalizer in test_finalizers.into_iter().rev() {
            finalizer.run(self.context, py);
        }

        self.clean_up_scope(py, FixtureScope::Function);

        passed
    }

    /// Execute a fixture
    fn execute_fixture(
        &self,
        py: Python<'_>,
        fixture: &NormalizedFixture,
    ) -> Option<(Py<PyAny>, Option<Finalizer>)> {
        if let Some(cached) = self
            .fixture_cache
            .get(fixture.function_name(), fixture.scope())
        {
            return Some((cached, None));
        }

        let mut fixture_arguments = HashMap::new();

        for fixture in fixture.dependencies() {
            let (value, finalizer) = self.execute_fixture(py, fixture)?;

            // Dependency finalizers are always added to the cache
            // They need to outlive the current fixture execution
            if let Some(finalizer) = finalizer {
                self.finalizer_cache.add_finalizer(finalizer);
            }

            fixture_arguments.insert(fixture.function_name().to_string(), value);
        }

        // For builtin fixtures, the value is stored directly in the function field
        // and function_definition is None. Return the value directly without calling.
        let result = match fixture.value() {
            NormalizedFixtureValue::Computed(value) => Ok(value.clone()),

            NormalizedFixtureValue::Function(function) => {
                let kwargs_dict = PyDict::new(py);

                for (key, value) in &fixture_arguments {
                    let _ = kwargs_dict.set_item(key.clone(), value);
                }

                if let Some(function_def) = fixture.stmt_function_def() {
                    let required = function_def.required_fixtures(py);

                    if required.contains(&"request".to_string()) {
                        let param_value = fixture
                            .param()
                            .and_then(|param| param.values.first())
                            .cloned();

                        if let Ok(request_obj) = Py::new(py, FixtureRequest::new(param_value)) {
                            kwargs_dict.set_item("request", request_obj).ok();
                        }
                    }
                }

                if kwargs_dict.is_empty() {
                    function.call0(py)
                } else {
                    function.call(py, (), Some(&kwargs_dict))
                }
            }
        };

        let handle_fixture_error = |err: PyErr| {
            let Some(fixture) = fixture.as_user_defined() else {
                // Assume that builtin fixtures don't fail
                return;
            };

            let missing_args =
                missing_arguments_from_error(fixture.name.function_name(), &err.to_string());

            if missing_args.is_empty() {
                report_fixture_failure(
                    self.context,
                    py,
                    source_file(fixture.module_path()),
                    &fixture.stmt_function_def,
                    &fixture_arguments,
                    &err,
                );
            } else {
                report_missing_fixtures(
                    self.context,
                    source_file(fixture.module_path()),
                    &fixture.stmt_function_def,
                    &missing_args,
                    FunctionKind::Fixture,
                );
            }
        };

        let result = match result {
            Ok(result) => result,
            Err(err) => {
                handle_fixture_error(err);
                return None;
            }
        };

        // If this is a generator fixture, we need to call next() to get the actual value
        // and create a finalizer for cleanup
        let (final_result, finalizer) = if let Some(user_defined_fixture) =
            fixture.as_user_defined()
            && fixture.is_generator()
        {
            let bound_result = result.bind(py);
            if let Ok(py_iter_bound) = bound_result.cast::<PyIterator>() {
                let py_iter: Py<PyIterator> = py_iter_bound.clone().unbind();
                let mut iterator = py_iter.bind(py).clone();
                match iterator.next() {
                    Some(Ok(value)) => {
                        let finalizer = {
                            Finalizer {
                                fixture_return: py_iter,
                                scope: fixture.scope(),
                                fixture_name: Some(user_defined_fixture.name.clone()),
                                stmt_function_def: Some(
                                    user_defined_fixture.stmt_function_def.clone(),
                                ),
                            }
                        };

                        (value.unbind(), Some(finalizer))
                    }
                    Some(Err(err)) => {
                        handle_fixture_error(err);
                        return None;
                    }
                    None => (result, None),
                }
            } else {
                (result, None)
            }
        } else if let Some(builtin_fixture) = fixture.as_builtin()
            && let Some(finalizer_fn) = &builtin_fixture.finalizer
        {
            // This code is quite hacky and should be revisited.
            //
            // We synthesize a finalizer by creating a new function that calls our generated finalizer function.

            let finalizer_fn_clone = finalizer_fn.clone();
            let result_clone = result.clone();

            let code = r"
def _builtin_finalizer(value, finalizer):
    yield value
    finalizer()
            ";

            let locals = PyDict::new(py);
            if py
                .run(&std::ffi::CString::new(code).unwrap(), None, Some(&locals))
                .is_ok()
                && let Ok(Some(gen_fn)) = locals.get_item("_builtin_finalizer")
                && let Ok(generator) = gen_fn.call1((result_clone, finalizer_fn_clone))
                && let Ok(py_iter) = generator.cast::<PyIterator>()
            {
                let py_iter_unbound = py_iter.clone().unbind();
                let mut iterator = py_iter.clone();

                if let Some(Ok(value)) = iterator.next() {
                    let finalizer = Finalizer {
                        fixture_return: py_iter_unbound,
                        scope: builtin_fixture.scope,
                        fixture_name: None,
                        stmt_function_def: None,
                    };

                    (value.unbind(), Some(finalizer))
                } else {
                    (result, None)
                }
            } else {
                (result, None)
            }
        } else {
            (result, None)
        };

        // Cache the result
        self.fixture_cache.insert(
            fixture.function_name().to_string(),
            final_result.clone(),
            fixture.scope(),
        );

        // Handle finalizer based on scope
        // Function-scoped finalizers are returned to be run immediately after the test
        // Higher-scoped finalizers are added to the cache
        let return_finalizer = finalizer.map_or_else(
            || None,
            |f| {
                if f.scope == FixtureScope::Function {
                    Some(f)
                } else {
                    self.finalizer_cache.add_finalizer(f);
                    None
                }
            },
        );

        Some((final_result, return_finalizer))
    }

    /// Cleans up the fixtures and finalizers for a given scope.
    ///
    /// This should be run after the given scope has finished execution.
    fn clean_up_scope(&self, py: Python, scope: FixtureScope) {
        self.finalizer_cache
            .run_and_clear_scope(self.context, py, scope);

        self.fixture_cache.clear_fixtures(scope);
    }

    /// Executes the fixtures for a given scope.
    ///
    /// Helper function used at the beginning of a scope to execute auto use fixture.
    fn execute_fixtures(&self, py: Python, fixture: &[NormalizedFixture]) {
        for fixture in fixture {
            if let Some((_, Some(finalizer))) = self.execute_fixture(py, fixture) {
                self.finalizer_cache.add_finalizer(finalizer);
            }
        }
    }
}
