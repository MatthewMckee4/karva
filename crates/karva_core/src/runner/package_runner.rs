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
        tags::skip::{extract_skip_reason, is_skip_exception},
    },
    normalize::{NormalizedModule, NormalizedPackage, NormalizedTestFunction},
    runner::{FinalizerCache, FixtureCache},
    utils::{full_test_name, source_file},
};

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

    fn clean_up(&self, py: Python, scope: FixtureScope) {
        self.finalizer_cache
            .run_and_clear_scope(self.context, py, scope);

        self.fixture_cache.clear_fixtures(scope);
    }

    pub(crate) fn run(&self, py: Python<'_>, session: &NormalizedPackage) {
        for fixture in &session.auto_use_fixtures {
            if let Some((_, Some(finalizer))) = self.execute_normalized_fixture(py, fixture) {
                self.finalizer_cache.add_finalizer(finalizer);
            }
        }

        self.run_package(py, session);

        self.clean_up(py, FixtureScope::Session);
    }

    fn run_module(&self, py: Python<'_>, module: &NormalizedModule) -> bool {
        for fixture in &module.auto_use_fixtures {
            if let Some((_, Some(finalizer))) = self.execute_normalized_fixture(py, fixture) {
                self.finalizer_cache.add_finalizer(finalizer);
            }
        }

        let mut passed = true;

        for test_function in &module.test_functions {
            passed &= self.run_normalized_test(py, test_function);

            if self.context.project().options().fail_fast() && !passed {
                break;
            }
        }

        self.clean_up(py, FixtureScope::Module);

        passed
    }

    fn run_package(&self, py: Python<'_>, package: &NormalizedPackage) -> bool {
        for fixture in &package.auto_use_fixtures {
            if let Some((_, Some(finalizer))) = self.execute_normalized_fixture(py, fixture) {
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

        if !self.context.project().options().fail_fast() || passed {
            for sub_package in package.packages.values() {
                passed &= self.run_package(py, sub_package);

                if self.context.project().options().fail_fast() && !passed {
                    break;
                }
            }
        }

        self.clean_up(py, FixtureScope::Package);

        passed
    }

    /// Run a normalized test function.
    fn run_normalized_test(&self, py: Python<'_>, test_fn: &NormalizedTestFunction) -> bool {
        tracing::info!("Running test {}", test_fn.name);

        // Check if test should be skipped
        if let (true, reason) = test_fn.tags.should_skip() {
            let reporter = self.context.reporter();

            self.context.result().register_test_case_result(
                &test_fn.name.to_string(),
                IndividualTestResultKind::Skipped { reason },
                Some(reporter),
            );
            return true;
        }

        // Check if test is expected to fail
        let expect_fail_tag = test_fn.tags.expect_fail_tag();
        let expect_fail = expect_fail_tag
            .as_ref()
            .is_some_and(crate::extensions::tags::expect_fail::ExpectFailTag::should_expect_fail);

        let mut test_finalizers = Vec::new();

        // Execute use_fixtures (for side effects only, don't pass to test)
        for fixture in &test_fn.use_fixture_dependencies {
            if let Some((_, Some(finalizer))) = self.execute_normalized_fixture(py, fixture) {
                test_finalizers.push(finalizer);
            }
        }

        // Execute regular fixture dependencies and add them to kwargs
        let mut kwargs = HashMap::new();
        for fixture in &test_fn.fixture_dependencies {
            if let Some((value, finalizer)) = self.execute_normalized_fixture(py, fixture) {
                kwargs.insert(fixture.function_name(), value);
                if let Some(finalizer) = finalizer {
                    test_finalizers.push(finalizer);
                }
            }
        }

        // Add test params to kwargs
        for (key, value) in &test_fn.params {
            kwargs.insert(key, value.clone());
        }

        let full_test_name = full_test_name(py, test_fn.name.to_string(), &kwargs);

        for fixture in &test_fn.auto_use_fixtures {
            if let Some((_, Some(finalizer))) = self.execute_normalized_fixture(py, fixture) {
                self.finalizer_cache.add_finalizer(finalizer);
            }
        }

        // Call the test function
        let test_result = if kwargs.is_empty() {
            test_fn.function.call0(py)
        } else {
            let py_dict = PyDict::new(py);
            for (key, value) in &kwargs {
                let _ = py_dict.set_item(key, value);
            }
            test_fn.function.call(py, (), Some(&py_dict))
        };

        let passed = match test_result {
            Ok(_) => {
                if expect_fail {
                    let reason = expect_fail_tag.and_then(|tag| tag.reason());

                    report_test_pass_on_expect_failure(
                        self.context,
                        source_file(test_fn.module_path()),
                        &test_fn.stmt_function_def,
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
                    let missing_args = missing_arguments_from_error(
                        test_fn.name.function_name(),
                        &err.to_string(),
                    );

                    if missing_args.is_empty() {
                        report_test_failure(
                            self.context,
                            py,
                            source_file(test_fn.module_path()),
                            &test_fn.stmt_function_def,
                            &kwargs,
                            &err,
                        );
                    } else {
                        report_missing_fixtures(
                            self.context,
                            source_file(test_fn.module_path()),
                            &test_fn.stmt_function_def,
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

        // Run function-scoped finalizers for this test in reverse order (LIFO)
        for finalizer in test_finalizers.into_iter().rev() {
            finalizer.run(self.context, py);
        }

        self.clean_up(py, FixtureScope::Function);

        passed
    }

    /// Execute a normalized fixture and all its dependencies, returning the fixture value and optional finalizer.
    fn execute_normalized_fixture(
        &self,
        py: Python<'_>,
        fixture: &NormalizedFixture,
    ) -> Option<(Py<PyAny>, Option<Finalizer>)> {
        if let Some(cached) = self
            .fixture_cache
            .get(fixture.function_name(), fixture.scope())
        {
            // Cached fixtures have already had their finalizers stored/run
            return Some((cached, None));
        }

        // Execute dependencies first
        let mut dep_kwargs = HashMap::new();
        for dep in fixture.dependencies() {
            let (dep_value, finalizer) = self.execute_normalized_fixture(py, dep)?;

            // Dependency finalizers are always added to the cache
            // They need to outlive the current fixture execution
            if let Some(finalizer) = finalizer {
                self.finalizer_cache.add_finalizer(finalizer);
            }

            dep_kwargs.insert(dep.function_name(), dep_value);
        }

        // Build kwargs for this fixture
        let kwargs_dict = PyDict::new(py);
        for (key, value) in &dep_kwargs {
            kwargs_dict.set_item(key, value).ok();
        }

        // For builtin fixtures, the value is stored directly in the function field
        // and function_definition is None. Return the value directly without calling.
        let result = match fixture.value() {
            NormalizedFixtureValue::Computed(value) => Ok(value.clone()),

            NormalizedFixtureValue::Function(function) => {
                if let Some(function_def) = fixture.stmt_function_def() {
                    let required = function_def.required_fixtures(py);

                    if required.contains(&"request".to_string()) {
                        // Create FixtureRequest with param (or None if not parametrized)
                        let param_value = fixture.param().map_or_else(|| py.None(), Clone::clone);

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
                    &dep_kwargs,
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
            // Built-in fixtures with finalizers (like monkeypatch)
            // We create a simple finalizer that calls the finalizer function
            let finalizer_fn_clone = finalizer_fn.clone();
            let result_clone = result.clone();

            // Create a Python generator that yields the result and then calls the finalizer
            let code = r"
def _builtin_finalizer(value, finalizer):
    yield value
    finalizer()
_builtin_finalizer
";

            let locals = PyDict::new(py);
            if py
                .run(&std::ffi::CString::new(code).unwrap(), None, Some(&locals))
                .is_ok()
                && let Ok(Some(gen_fn)) = locals.get_item("_builtin_finalizer")
                && let Ok(generator) = gen_fn.call1((result_clone, finalizer_fn_clone))
                && let Ok(py_iter) = generator.cast::<PyIterator>()
            {
                let py_iter_unbound: Py<PyIterator> = py_iter.clone().unbind();
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
}
