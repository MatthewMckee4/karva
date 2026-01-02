use std::collections::HashMap;
use std::sync::Arc;

use karva_diagnostic::IndividualTestResultKind;
use karva_python_semantic::{FunctionKind, QualifiedTestName};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyIterator};
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::Context;
use crate::diagnostic::{
    report_fixture_failure, report_missing_fixtures, report_test_failure,
    report_test_pass_on_expect_failure,
};
use crate::extensions::fixtures::{
    Finalizer, FixtureScope, NormalizedFixture, create_fixture_with_finalizer,
    missing_arguments_from_error,
};
use crate::extensions::tags::expect_fail::ExpectFailTag;
use crate::extensions::tags::skip::{extract_skip_reason, is_skip_exception};
use crate::normalize::{NormalizedModule, NormalizedPackage, NormalizedTest};
use crate::runner::{FinalizerCache, FixtureCache};
use crate::utils::{full_test_name, source_file};

/// A struct that is used to execute tests within a package.
///
/// We assume a normalized state of the package.
pub struct NormalizedPackageRunner<'ctx, 'a> {
    context: &'ctx Context<'a>,
    fixture_cache: FixtureCache,
    finalizer_cache: FinalizerCache,
}

impl<'ctx, 'a> NormalizedPackageRunner<'ctx, 'a> {
    pub(crate) fn new(context: &'ctx Context<'a>) -> Self {
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
        let auto_use_errors = self.run_fixtures(py, &session.auto_use_fixtures);

        for error in auto_use_errors {
            report_fixture_failure(self.context, py, error);
        }

        self.execute_package(py, session);

        self.clean_up_scope(py, FixtureScope::Session);
    }

    /// Execute a module.
    ///
    /// Executes all tests in a module.
    ///
    /// Failing fast if the user has specified that we should.
    fn execute_module(&self, py: Python<'_>, module: NormalizedModule) -> bool {
        let auto_use_errors = self.run_fixtures(py, &module.auto_use_fixtures);

        for error in auto_use_errors {
            report_fixture_failure(self.context, py, error);
        }

        let mut passed = true;

        for test_function in module.test_functions {
            passed &= self.execute_test(py, test_function);

            if self.context.settings().test().fail_fast && !passed {
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
            path: _package_path,
            modules,
            packages,
            auto_use_fixtures,
        } = package;

        let auto_use_errors = self.run_fixtures(py, &auto_use_fixtures);

        for error in auto_use_errors {
            report_fixture_failure(self.context, py, error);
        }

        let mut passed = true;

        for module in modules.into_values() {
            passed &= self.execute_module(py, module);

            if self.context.settings().test().fail_fast && !passed {
                break;
            }
        }

        if !self.context.settings().test().fail_fast || passed {
            for sub_package in packages.into_values() {
                passed &= self.execute_package(py, sub_package);

                if self.context.settings().test().fail_fast && !passed {
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
            tags: _test_tags,
            stmt_function_def,
        } = test;

        if let (true, reason) = tags.should_skip() {
            return self.context.register_test_case_result(
                &QualifiedTestName::new(name, None),
                IndividualTestResultKind::Skipped { reason },
                std::time::Duration::ZERO,
            );
        }

        let start_time = std::time::Instant::now();

        let expect_fail_tag = tags.expect_fail_tag();
        let expect_fail = expect_fail_tag
            .as_ref()
            .is_some_and(ExpectFailTag::should_expect_fail);

        let mut test_finalizers = Vec::new();

        let mut fixture_call_errors = Vec::new();

        let use_fixture_errors = self.run_fixtures(py, &use_fixture_dependencies);

        fixture_call_errors.extend(use_fixture_errors);

        let mut function_arguments = HashMap::new();

        for fixture in &fixture_dependencies {
            match self.run_fixture(py, fixture) {
                Ok((value, finalizer)) => {
                    function_arguments.insert(fixture.function_name().to_string(), value);

                    if let Some(finalizer) = finalizer {
                        test_finalizers.push(finalizer);
                    }
                }
                Err(err) => {
                    fixture_call_errors.push(err);
                }
            }
        }

        let auto_use_errors = self.run_fixtures(py, &auto_use_fixtures);
        fixture_call_errors.extend(auto_use_errors);

        for (key, value) in params {
            function_arguments.insert(key, value);
        }

        let full_test_name = full_test_name(py, name.to_string(), &function_arguments);

        let full_test_name = QualifiedTestName::new(name.clone(), Some(full_test_name));

        tracing::debug!("Running test `{}`", full_test_name);

        let test_result = if function_arguments.is_empty() {
            function.call0(py)
        } else {
            let py_dict = PyDict::new(py);
            for (key, value) in &function_arguments {
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
                        source_file(self.context.system(), &test_module_path),
                        &stmt_function_def,
                        reason,
                    );

                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Failed,
                        start_time.elapsed(),
                    )
                } else {
                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Passed,
                        start_time.elapsed(),
                    )
                }
            }
            Err(err) => {
                if is_skip_exception(py, &err) {
                    let reason = extract_skip_reason(py, &err);
                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Skipped { reason },
                        start_time.elapsed(),
                    )
                } else if expect_fail {
                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Passed,
                        start_time.elapsed(),
                    )
                } else {
                    let missing_args =
                        missing_arguments_from_error(name.function_name(), &err.to_string());

                    if missing_args.is_empty() {
                        report_test_failure(
                            self.context,
                            py,
                            source_file(self.context.system(), &test_module_path),
                            &stmt_function_def,
                            &function_arguments,
                            &err,
                        );
                    } else {
                        report_missing_fixtures(
                            self.context,
                            py,
                            source_file(self.context.system(), &test_module_path),
                            &stmt_function_def,
                            &missing_args,
                            FunctionKind::Test,
                            fixture_call_errors,
                        );
                    }

                    self.context.register_test_case_result(
                        &full_test_name,
                        IndividualTestResultKind::Failed,
                        start_time.elapsed(),
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

    /// Run a fixture
    #[allow(clippy::result_large_err)]
    fn run_fixture(
        &self,
        py: Python<'_>,
        fixture: &NormalizedFixture,
    ) -> Result<(Py<PyAny>, Option<Finalizer>), FixtureCallError> {
        if let Some(cached) = self
            .fixture_cache
            .get(fixture.function_name(), fixture.scope())
        {
            return Ok((cached, None));
        }

        let mut function_arguments = HashMap::new();

        for fixture in fixture.dependencies() {
            match self.run_fixture(py, fixture) {
                Ok((value, finalizer)) => {
                    function_arguments.insert(fixture.function_name().to_string(), value);

                    if let Some(finalizer) = finalizer {
                        self.finalizer_cache.add_finalizer(finalizer);
                    }
                }
                Err(err) => {
                    return Err(err);
                }
            }
        }

        let fixture_call_result = match fixture.call(py, &function_arguments) {
            Ok(fixture_call_result) => fixture_call_result,
            Err(err) => {
                let fixture_def = fixture
                    .as_user_defined()
                    .expect("builtin fixtures to not fail");

                return Err(FixtureCallError {
                    fixture_name: fixture_def.name.function_name().to_string(),
                    error: err,
                    stmt_function_def: fixture_def.stmt_function_def.clone(),
                    source_file: source_file(
                        self.context.system(),
                        fixture_def.name.module_path().path(),
                    ),
                    arguments: function_arguments.clone(),
                });
            }
        };

        let (final_result, finalizer) =
            match get_value_and_finalizer(py, fixture, fixture_call_result) {
                Ok((final_result, finalizer)) => (final_result, finalizer),
                Err(err) => {
                    let fixture_def = fixture
                        .as_user_defined()
                        .expect("builtin fixtures to not fail");

                    return Err(FixtureCallError {
                        fixture_name: fixture_def.name.function_name().to_string(),
                        error: err,
                        stmt_function_def: fixture_def.stmt_function_def.clone(),
                        source_file: source_file(
                            self.context.system(),
                            fixture_def.name.module_path().path(),
                        ),
                        arguments: function_arguments.clone(),
                    });
                }
            };

        if fixture.is_user_defined() {
            // Cache the result
            self.fixture_cache.insert(
                fixture.function_name().to_string(),
                final_result.clone(),
                fixture.scope(),
            );
        }

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

        Ok((final_result, return_finalizer))
    }

    /// Cleans up the fixtures and finalizers for a given scope.
    ///
    /// This should be run after the given scope has finished execution.
    fn clean_up_scope(&self, py: Python, scope: FixtureScope) {
        self.finalizer_cache
            .run_and_clear_scope(self.context, py, scope);

        self.fixture_cache.clear_fixtures(scope);
    }

    /// Runs the fixtures for a given scope.
    ///
    /// Helper function used at the beginning of a scope to execute auto use fixture.
    /// Here, we do nothing with the result.
    fn run_fixtures<P: std::ops::Deref<Target = NormalizedFixture>>(
        &self,
        py: Python,
        fixtures: &[P],
    ) -> Vec<FixtureCallError> {
        let mut errors = Vec::new();
        for fixture in fixtures {
            match self.run_fixture(py, fixture) {
                Ok((_, finalizer)) => {
                    if let Some(finalizer) = finalizer {
                        self.finalizer_cache.add_finalizer(finalizer);
                    }
                }
                Err(error) => errors.push(error),
            }
        }

        errors
    }
}

fn get_value_and_finalizer(
    py: Python<'_>,
    fixture: &NormalizedFixture,
    fixture_call_result: Py<PyAny>,
) -> PyResult<(Py<PyAny>, Option<Finalizer>)> {
    // If this is a generator fixture, we need to call next() to get the actual value
    // and create a finalizer for cleanup
    if let Some(user_defined_fixture) = fixture.as_user_defined()
        && user_defined_fixture.is_generator
        && let Ok(mut bound_iterator) = fixture_call_result
            .clone()
            .into_bound(py)
            .cast_into::<PyIterator>()
    {
        match bound_iterator.next() {
            Some(Ok(value)) => {
                let py_iter = bound_iterator.clone().unbind();
                let finalizer = {
                    Finalizer {
                        fixture_return: py_iter,
                        scope: fixture.scope(),
                        fixture_name: Some(user_defined_fixture.name.clone()),
                        stmt_function_def: Some(user_defined_fixture.stmt_function_def.clone()),
                    }
                };

                Ok((value.unbind(), Some(finalizer)))
            }
            Some(Err(err)) => Err(err),
            None => unreachable!(),
        }
    } else if let Some(builtin_fixture) = fixture.as_builtin()
        && let Some(finalizer_fn) = &builtin_fixture.finalizer
        && let Ok(mut bound_iterator) =
            create_fixture_with_finalizer(py, &fixture_call_result, finalizer_fn)
        && let Some(Ok(value)) = bound_iterator.next()
    {
        let py_iter_unbound = bound_iterator.clone().unbind();
        let finalizer = Finalizer {
            fixture_return: py_iter_unbound,
            scope: builtin_fixture.scope,
            fixture_name: None,
            stmt_function_def: None,
        };

        Ok((value.unbind(), Some(finalizer)))
    } else {
        Ok((fixture_call_result, None))
    }
}

pub struct FixtureCallError {
    pub fixture_name: String,
    pub error: PyErr,
    pub stmt_function_def: Arc<StmtFunctionDef>,
    pub source_file: SourceFile,
    pub arguments: HashMap<String, Py<PyAny>>,
}
