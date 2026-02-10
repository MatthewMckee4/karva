use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

type FixtureArguments = HashMap<String, Py<PyAny>>;

use karva_diagnostic::IndividualTestResultKind;
use karva_python_semantic::{FunctionKind, QualifiedFunctionName, QualifiedTestName};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyIterator};
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::Context;
use crate::diagnostic::{
    report_fixture_failure, report_missing_fixtures, report_test_failure,
    report_test_pass_on_expect_failure,
};
use crate::discovery::{DiscoveredModule, DiscoveredPackage};
use crate::extensions::fixtures::{
    Finalizer, FixtureScope, NormalizedFixture, create_fixture_with_finalizer,
    missing_arguments_from_error,
};
use crate::extensions::tags::expect_fail::ExpectFailTag;
use crate::extensions::tags::skip::{extract_skip_reason, is_skip_exception};
use crate::runner::fixture_resolver::RuntimeFixtureResolver;
use crate::runner::test_iterator::{TestVariant, TestVariantIterator};
use crate::runner::{FinalizerCache, FixtureCache};
use crate::utils::{full_test_name, source_file};

/// Executes discovered tests within a package hierarchy.
///
/// Manages fixture caching and finalization across different scopes
/// (function, module, package, session) during test execution.
/// Fixtures are resolved at runtime rather than pre-computed.
pub struct PackageRunner<'ctx, 'a> {
    /// Reference to the test execution context.
    context: &'ctx Context<'a>,

    /// Cache for fixture values to avoid re-computation within a scope.
    fixture_cache: FixtureCache,

    /// Cache for fixture finalizers to run cleanup at appropriate times.
    finalizer_cache: FinalizerCache,
}

impl<'ctx, 'a> PackageRunner<'ctx, 'a> {
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
    pub(crate) fn execute(&self, py: Python<'_>, session: &DiscoveredPackage) {
        // Get session-scoped auto-use fixtures
        if let Some(config_module) = session.configuration_module_impl() {
            let mut resolver = RuntimeFixtureResolver::new(&[], config_module);
            let session_auto_use_fixtures =
                resolver.get_normalized_auto_use_fixtures(py, FixtureScope::Session);
            let auto_use_errors = self.run_fixtures(py, &session_auto_use_fixtures);
            for error in auto_use_errors {
                report_fixture_failure(self.context, py, error);
            }
        }

        self.execute_package(py, session, &[]);

        self.clean_up_scope(py, FixtureScope::Session);
    }

    /// Execute a module.
    ///
    /// Executes all tests in a module.
    ///
    /// Failing fast if the user has specified that we should.
    fn execute_module(
        &self,
        py: Python<'_>,
        module: &DiscoveredModule,
        parents: &[&DiscoveredPackage],
    ) -> bool {
        let mut resolver = RuntimeFixtureResolver::new(parents, module);

        // Run module-scoped auto-use fixtures
        let module_auto_use_fixtures =
            resolver.get_normalized_auto_use_fixtures(py, FixtureScope::Module);
        let auto_use_errors = self.run_fixtures(py, &module_auto_use_fixtures);

        for error in auto_use_errors {
            report_fixture_failure(self.context, py, error);
        }

        let mut passed = true;

        for test_function in module.test_functions() {
            // Create a new resolver for each test to handle fixture resolution
            let mut test_resolver = RuntimeFixtureResolver::new(parents, module);

            // Iterate over all test variants (parametrize combinations × fixture combinations)
            for variant in TestVariantIterator::new(py, test_function, &mut test_resolver) {
                passed &= self.execute_test_variant(py, variant);

                if self.context.settings().test().fail_fast && !passed {
                    break;
                }
            }

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
    fn execute_package(
        &self,
        py: Python<'_>,
        package: &DiscoveredPackage,
        parents: &[&DiscoveredPackage],
    ) -> bool {
        let mut new_parents = parents.to_vec();
        new_parents.push(package);

        // Run package-scoped auto-use fixtures
        if let Some(config_module) = package.configuration_module_impl() {
            let mut resolver = RuntimeFixtureResolver::new(parents, config_module);
            let package_auto_use_fixtures =
                resolver.get_normalized_auto_use_fixtures(py, FixtureScope::Package);
            let auto_use_errors = self.run_fixtures(py, &package_auto_use_fixtures);
            for error in auto_use_errors {
                report_fixture_failure(self.context, py, error);
            }
        }

        let mut passed = true;

        for module in package.modules().values() {
            passed &= self.execute_module(py, module, &new_parents);

            if self.context.settings().test().fail_fast && !passed {
                break;
            }
        }

        if !self.context.settings().test().fail_fast || passed {
            for sub_package in package.packages().values() {
                passed &= self.execute_package(py, sub_package, &new_parents);

                if self.context.settings().test().fail_fast && !passed {
                    break;
                }
            }
        }

        self.clean_up_scope(py, FixtureScope::Package);

        passed
    }

    /// Check if a test variant should be skipped based on filters and tags.
    ///
    /// Returns `Some(result)` if the test should be skipped (with the registered result),
    /// or `None` if the test should proceed.
    fn should_skip_variant(
        &self,
        name: &QualifiedFunctionName,
        tags: &crate::extensions::tags::Tags,
    ) -> Option<bool> {
        let tag_filter = &self.context.settings().test().tag_filter;
        if !tag_filter.is_empty() {
            let custom_names = tags.custom_tag_names();
            if !tag_filter.matches(&custom_names) {
                return Some(self.context.register_test_case_result(
                    &QualifiedTestName::new(name.clone(), None),
                    IndividualTestResultKind::Skipped { reason: None },
                    std::time::Duration::ZERO,
                ));
            }
        }

        let name_filter = &self.context.settings().test().name_filter;
        if !name_filter.is_empty() {
            let display_name = QualifiedTestName::new(name.clone(), None).to_string();
            if !name_filter.matches(&display_name) {
                return Some(self.context.register_test_case_result(
                    &QualifiedTestName::new(name.clone(), None),
                    IndividualTestResultKind::Skipped { reason: None },
                    std::time::Duration::ZERO,
                ));
            }
        }

        if let (true, reason) = tags.should_skip() {
            return Some(self.context.register_test_case_result(
                &QualifiedTestName::new(name.clone(), None),
                IndividualTestResultKind::Skipped { reason },
                std::time::Duration::ZERO,
            ));
        }

        None
    }

    /// Resolve fixture dependencies and parametrize params into function arguments.
    fn setup_test_fixtures(
        &self,
        py: Python<'_>,
        fixture_dependencies: &[Rc<NormalizedFixture>],
        use_fixture_dependencies: &[Rc<NormalizedFixture>],
        auto_use_fixtures: &[Rc<NormalizedFixture>],
        params: HashMap<String, Arc<Py<PyAny>>>,
    ) -> (FixtureArguments, Vec<FixtureCallError>, Vec<Finalizer>) {
        let mut test_finalizers = Vec::new();
        let mut fixture_call_errors = Vec::new();

        let use_fixture_errors = self.run_fixtures(py, use_fixture_dependencies);
        fixture_call_errors.extend(use_fixture_errors);

        let mut function_arguments: FixtureArguments = HashMap::new();

        for fixture in fixture_dependencies {
            match self.run_fixture(py, fixture) {
                Ok((value, finalizer)) => {
                    function_arguments
                        .insert(fixture.function_name().to_string(), value.clone_ref(py));

                    if let Some(finalizer) = finalizer {
                        test_finalizers.push(finalizer);
                    }
                }
                Err(err) => {
                    fixture_call_errors.push(err);
                }
            }
        }

        let auto_use_errors = self.run_fixtures(py, auto_use_fixtures);
        fixture_call_errors.extend(auto_use_errors);

        // Add parametrize params to function arguments
        for (key, value) in params {
            function_arguments.insert(
                key,
                Arc::try_unwrap(value).unwrap_or_else(|arc| (*arc).clone_ref(py)),
            );
        }

        (function_arguments, fixture_call_errors, test_finalizers)
    }

    /// Classify a test result, handling `expect_fail` logic and error reporting.
    #[expect(clippy::too_many_arguments)]
    fn classify_test_result(
        &self,
        py: Python<'_>,
        test_result: PyResult<Py<PyAny>>,
        expect_fail: bool,
        expect_fail_tag: Option<ExpectFailTag>,
        qualified_test_name: &QualifiedTestName,
        name: &QualifiedFunctionName,
        test_module_path: &camino::Utf8PathBuf,
        stmt_function_def: &StmtFunctionDef,
        function_arguments: &FixtureArguments,
        fixture_call_errors: Vec<FixtureCallError>,
        start_time: std::time::Instant,
    ) -> bool {
        match test_result {
            Ok(_) => {
                if expect_fail {
                    let reason = expect_fail_tag.and_then(|tag| tag.reason());

                    report_test_pass_on_expect_failure(
                        self.context,
                        source_file(test_module_path),
                        stmt_function_def,
                        reason,
                    );

                    self.context.register_test_case_result(
                        qualified_test_name,
                        IndividualTestResultKind::Failed,
                        start_time.elapsed(),
                    )
                } else {
                    self.context.register_test_case_result(
                        qualified_test_name,
                        IndividualTestResultKind::Passed,
                        start_time.elapsed(),
                    )
                }
            }
            Err(err) => {
                if is_skip_exception(py, &err) {
                    let reason = extract_skip_reason(py, &err);
                    self.context.register_test_case_result(
                        qualified_test_name,
                        IndividualTestResultKind::Skipped { reason },
                        start_time.elapsed(),
                    )
                } else if expect_fail {
                    self.context.register_test_case_result(
                        qualified_test_name,
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
                            source_file(test_module_path),
                            stmt_function_def,
                            function_arguments,
                            &err,
                        );
                    } else {
                        report_missing_fixtures(
                            self.context,
                            py,
                            source_file(test_module_path),
                            stmt_function_def,
                            &missing_args,
                            FunctionKind::Test,
                            fixture_call_errors,
                        );
                    }

                    self.context.register_test_case_result(
                        qualified_test_name,
                        IndividualTestResultKind::Failed,
                        start_time.elapsed(),
                    )
                }
            }
        }
    }

    /// Run a test variant (a specific combination of parametrize values and fixtures).
    fn execute_test_variant(&self, py: Python<'_>, variant: TestVariant) -> bool {
        let tags = variant.resolved_tags();
        let test_module_path = variant.module_path().clone();

        let TestVariant {
            test,
            params,
            fixture_dependencies,
            use_fixture_dependencies,
            auto_use_fixtures,
            tags: _variant_tags,
        } = variant;

        let name = test.name.clone();
        let function = test.py_function.clone_ref(py);
        let stmt_function_def = Rc::clone(&test.stmt_function_def);

        if let Some(result) = self.should_skip_variant(&name, &tags) {
            return result;
        }

        let start_time = std::time::Instant::now();

        let expect_fail_tag = tags.expect_fail_tag();
        let expect_fail = expect_fail_tag
            .as_ref()
            .is_some_and(ExpectFailTag::should_expect_fail);

        let (function_arguments, fixture_call_errors, test_finalizers) = self.setup_test_fixtures(
            py,
            &fixture_dependencies,
            &use_fixture_dependencies,
            &auto_use_fixtures,
            params,
        );

        let computed_full_test_name = full_test_name(py, name.to_string(), &function_arguments);

        let qualified_test_name =
            QualifiedTestName::new(name.clone(), Some(computed_full_test_name));

        tracing::debug!("Running test `{}`", qualified_test_name);

        // Set snapshot context so `karva.assert_snapshot()` can determine the current test.
        // Use `function_name()` (not `qualified_test_name`) to avoid doubling the module prefix,
        // since `snapshot_path()` already prepends the module name from the file stem.
        let snapshot_test_name =
            full_test_name(py, name.function_name().to_string(), &function_arguments);
        crate::extensions::functions::snapshot::set_snapshot_context(
            test_module_path.to_string(),
            snapshot_test_name,
        );

        let py_dict = PyDict::new(py);
        for (key, value) in &function_arguments {
            let _ = py_dict.set_item(key, value.as_ref());
        }

        let run_test = || {
            if function_arguments.is_empty() {
                function.call0(py)
            } else {
                function.call(py, (), Some(&py_dict))
            }
        };

        let mut test_result = run_test();

        let mut retry_count = self.context.settings().test().retry;

        while retry_count > 0 {
            if test_result.is_ok() {
                break;
            }
            tracing::debug!("Retrying test `{}`", qualified_test_name);
            retry_count -= 1;
            test_result = run_test();
        }

        let passed = self.classify_test_result(
            py,
            test_result,
            expect_fail,
            expect_fail_tag,
            &qualified_test_name,
            &name,
            &test_module_path,
            &stmt_function_def,
            &function_arguments,
            fixture_call_errors,
            start_time,
        );

        for finalizer in test_finalizers.into_iter().rev() {
            finalizer.run(self.context, py);
        }

        self.clean_up_scope(py, FixtureScope::Function);

        passed
    }

    /// Run a fixture
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

        let mut function_arguments: FixtureArguments = HashMap::new();

        for fixture in fixture.dependencies() {
            match self.run_fixture(py, fixture) {
                Ok((value, finalizer)) => {
                    function_arguments
                        .insert(fixture.function_name().to_string(), value.clone_ref(py));

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
                    source_file: source_file(fixture_def.name.module_path().path()),
                    arguments: function_arguments,
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
                        source_file: source_file(fixture_def.name.module_path().path()),
                        arguments: HashMap::new(),
                    });
                }
            };

        if fixture.is_user_defined() {
            // Cache the result
            self.fixture_cache.insert(
                fixture.function_name().to_string(),
                final_result.clone_ref(py),
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
            .clone_ref(py)
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
            None => Err(pyo3::exceptions::PyRuntimeError::new_err(
                "Generator fixture yielded no value",
            )),
        }
    } else if let Some(builtin_fixture) = fixture.as_builtin()
        && let Some(finalizer_fn) = &builtin_fixture.finalizer
        && let Ok(mut bound_iterator) =
            create_fixture_with_finalizer(py, &fixture_call_result, finalizer_fn)
        && let Some(Ok(value)) = bound_iterator.next()
    {
        let py_iter_unbound = bound_iterator.unbind();
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
    pub(crate) fixture_name: String,
    pub(crate) error: PyErr,
    pub(crate) stmt_function_def: Rc<StmtFunctionDef>,
    pub(crate) source_file: SourceFile,
    pub(crate) arguments: FixtureArguments,
}
