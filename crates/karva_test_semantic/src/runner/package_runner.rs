use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

type FixtureArguments = HashMap<String, Py<PyAny>>;

use karva_diagnostic::IndividualTestResultKind;
use karva_metadata::RunIgnoredMode;
use karva_metadata::filter::EvalContext;
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
    Finalizer, FixtureScope, HasFixtures, NormalizedFixture, missing_arguments_from_error,
};
use crate::extensions::tags::expect_fail::ExpectFailTag;
use crate::extensions::tags::skip::{extract_skip_reason, is_skip_exception};
use crate::extensions::tags::timeout::TimeoutTag;
use crate::runner::fixture_resolver::RuntimeFixtureResolver;
use crate::runner::test_iterator::{TestVariant, TestVariantIterator};
use crate::runner::{FinalizerCache, FixtureCache};
use crate::utils::{
    full_test_name, run_coroutine, run_test_with_timeout, set_attempt_env, set_test_name_env,
    source_file,
};

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

    /// Running count of failed tests observed during this run.
    ///
    /// Used to enforce `--max-fail=N`: once this counter reaches the
    /// configured budget we stop scheduling new tests.
    failed_count: Cell<u32>,
}

impl<'ctx, 'a> PackageRunner<'ctx, 'a> {
    pub(crate) fn new(context: &'ctx Context<'a>) -> Self {
        Self {
            context,
            fixture_cache: FixtureCache::default(),
            finalizer_cache: FinalizerCache::default(),
            failed_count: Cell::new(0),
        }
    }

    /// Returns `true` when the configured `max-fail` limit has been reached,
    /// signalling that the runner should stop scheduling tests.
    fn max_fail_reached(&self) -> bool {
        self.context
            .settings()
            .test()
            .max_fail
            .is_exceeded_by(self.failed_count.get())
    }

    /// If the test exceeded the configured `slow-timeout`, register it as
    /// slow so the reporter emits a `SLOW` line ahead of the result line and
    /// the run summary includes a slow counter.
    fn maybe_register_slow(
        &self,
        test_name: &QualifiedTestName,
        total_duration: std::time::Duration,
    ) {
        if let Some(threshold) = self.context.settings().test().slow_timeout
            && total_duration > threshold
        {
            self.context.register_slow_test(test_name, total_duration);
        }
    }

    /// Record a test variant's outcome for `max-fail` accounting.
    fn record_outcome(&self, passed: bool) {
        if !passed {
            self.failed_count
                .set(self.failed_count.get().saturating_add(1));
        }
    }

    /// Executes all tests in a package.
    ///
    /// The main entrypoint for actual test execution.
    pub(crate) fn execute(&self, py: Python<'_>, session: &DiscoveredPackage) {
        // Resolve session-scoped auto-use fixtures using the session package
        // itself as the `HasFixtures` source so that the walk includes both
        // the user conftest at the session root and the framework module. No
        // `if let Some(...)` gate: the session always exists, and if neither
        // slot contributes any autouse fixtures the walk returns an empty vec.
        self.run_auto_use_fixtures(py, &[], session, FixtureScope::Session);

        self.execute_package(py, session, &[]);

        self.clean_up_scope(py, FixtureScope::Session);
    }

    /// Resolve and run auto-use fixtures for `scope`, reporting any failures
    /// through the standard fixture-failure diagnostic. The `current` source
    /// is whichever `HasFixtures` provider applies for this scope (the
    /// session package, a module, or a package configuration module).
    fn run_auto_use_fixtures<'b>(
        &self,
        py: Python<'_>,
        parents: &'b [&'b DiscoveredPackage],
        current: &'b (dyn HasFixtures<'b> + 'b),
        scope: FixtureScope,
    ) {
        let mut resolver = RuntimeFixtureResolver::new(parents, current);
        let auto_use_fixtures = resolver.get_normalized_auto_use_fixtures(py, scope);
        let auto_use_errors = self.run_fixtures(py, &auto_use_fixtures);
        for error in auto_use_errors {
            report_fixture_failure(self.context, py, error);
        }
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
        self.run_auto_use_fixtures(py, parents, module, FixtureScope::Module);

        let mut passed = true;

        for test_function in module.test_functions() {
            // Create a new resolver for each test to handle fixture resolution
            let mut test_resolver = RuntimeFixtureResolver::new(parents, module);

            // Iterate over all test variants (parametrize combinations × fixture combinations).
            for variant in TestVariantIterator::new(py, test_function, &mut test_resolver) {
                let variant_passed = self.execute_test_variant(py, variant);
                self.record_outcome(variant_passed);
                passed &= variant_passed;

                if self.max_fail_reached() {
                    break;
                }
            }

            // Tick once per test function (parametrize variants don't each
            // tick — `total_tests` counts function defs, so per-variant ticks
            // would overrun the bar).
            self.context
                .notify_test_function_completed(&QualifiedTestName::new(
                    test_function.name.clone(),
                    None,
                ));

            if self.max_fail_reached() {
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

        if let Some(config_module) = package.configuration_module_impl() {
            self.run_auto_use_fixtures(py, parents, config_module, FixtureScope::Package);
        }

        let mut passed = true;

        for module in package.modules().values() {
            passed &= self.execute_module(py, module, &new_parents);

            if self.max_fail_reached() {
                break;
            }
        }

        if !self.max_fail_reached() {
            for sub_package in package.packages().values() {
                passed &= self.execute_package(py, sub_package, &new_parents);

                if self.max_fail_reached() {
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
        let filter = &self.context.settings().test().filter;
        let run_ignored = self.context.settings().test().run_ignored;

        if !filter.is_empty() {
            let qualified = QualifiedTestName::new(name.clone(), None);
            let display_name = qualified.to_string();
            let custom_names = tags.custom_tag_names();
            let ctx = EvalContext {
                test_name: &display_name,
                tags: &custom_names,
            };
            if !filter.matches(&ctx) {
                return Some(self.context.register_test_case_result(
                    &qualified,
                    IndividualTestResultKind::Skipped { reason: None },
                    std::time::Duration::ZERO,
                ));
            }
        }

        match run_ignored {
            RunIgnoredMode::Default => {
                if let (true, reason) = tags.should_skip() {
                    return Some(self.context.register_test_case_result(
                        &QualifiedTestName::new(name.clone(), None),
                        IndividualTestResultKind::Skipped { reason },
                        std::time::Duration::ZERO,
                    ));
                }
            }
            RunIgnoredMode::Only => {
                // Skip tests whose skip condition is not active; only tests
                // that would actually be skipped in a normal run are included.
                if let (false, _) = tags.should_skip() {
                    return Some(self.context.register_test_case_result(
                        &QualifiedTestName::new(name.clone(), None),
                        IndividualTestResultKind::Skipped { reason: None },
                        std::time::Duration::ZERO,
                    ));
                }
            }
            RunIgnoredMode::All => {
                // run everything regardless of skip tags
            }
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

    /// Classify a test result, handling `expect_fail` logic and error
    /// reporting. The provided `register` closure is invoked exactly once
    /// with the final [`IndividualTestResultKind`] so the caller can choose
    /// between `register_test_case_result` (for non-retried tests) and
    /// `register_retried_result` (for retried tests).
    fn classify_test_result(
        &self,
        py: Python<'_>,
        test_result: PyResult<Py<PyAny>>,
        fixture_call_errors: Vec<FixtureCallError>,
        ctx: &VariantReportCtx<'_>,
        register: impl FnOnce(IndividualTestResultKind) -> bool,
    ) -> bool {
        let expect_fail = ctx
            .expect_fail_tag
            .as_ref()
            .is_some_and(ExpectFailTag::should_expect_fail);

        let err = match test_result {
            Ok(_) if expect_fail => {
                let reason = ctx.expect_fail_tag.as_ref().and_then(ExpectFailTag::reason);
                report_test_pass_on_expect_failure(
                    self.context,
                    source_file(ctx.test_module_path),
                    ctx.stmt_function_def,
                    reason,
                );
                return register(IndividualTestResultKind::Failed);
            }
            Ok(_) => return register(IndividualTestResultKind::Passed),
            Err(err) => err,
        };

        if is_skip_exception(py, &err) {
            return register(IndividualTestResultKind::Skipped {
                reason: extract_skip_reason(py, &err),
            });
        }

        if expect_fail {
            return register(IndividualTestResultKind::Passed);
        }

        let missing_args = missing_arguments_from_error(ctx.name.function_name(), &err.to_string());

        if missing_args.is_empty() {
            report_test_failure(
                self.context,
                py,
                source_file(ctx.test_module_path),
                ctx.stmt_function_def,
                ctx.function_arguments,
                &err,
            );
        } else {
            report_missing_fixtures(
                self.context,
                py,
                source_file(ctx.test_module_path),
                ctx.stmt_function_def,
                &missing_args,
                FunctionKind::Test,
                fixture_call_errors,
            );
        }

        register(IndividualTestResultKind::Failed)
    }

    /// Drive the test closure with the configured retry budget.
    ///
    /// Emits a per-attempt report after every failed retry and, when at
    /// least one retry occurred, after the final attempt as well, so the
    /// reporter sees the same `TRY N PASS|FAIL` ordering as nextest.
    fn run_with_retries(
        &self,
        py: Python<'_>,
        qualified_test_name: &QualifiedTestName,
        configured_retries: u32,
        mut run_test: impl FnMut() -> PyResult<Py<PyAny>>,
    ) -> RetryOutcome {
        let max_attempts = configured_retries.saturating_add(1);

        let mut attempt: u32 = 1;
        let _ = set_attempt_env(py, attempt, max_attempts);
        let mut attempt_start = std::time::Instant::now();
        let mut test_result = run_test();

        let mut retry_count = configured_retries;
        let mut was_retried = false;
        let mut final_attempt_duration = attempt_start.elapsed();

        while retry_count > 0 {
            if test_result.is_ok() {
                break;
            }
            let attempt_duration = attempt_start.elapsed();
            self.context.report_test_attempt(
                qualified_test_name,
                attempt,
                IndividualTestResultKind::Failed,
                attempt_duration,
            );
            was_retried = true;

            tracing::debug!("Retrying test `{}`", qualified_test_name);
            retry_count -= 1;
            attempt += 1;
            let _ = set_attempt_env(py, attempt, max_attempts);
            attempt_start = std::time::Instant::now();
            test_result = run_test();
            final_attempt_duration = attempt_start.elapsed();
        }

        if was_retried {
            // Emit the per-attempt line for the final attempt so output
            // ordering matches nextest:
            //   TRY 1 FAIL ...
            //   TRY 2 PASS ...   (or TRY 2 FAIL for an exhausted retry)
            // The diagnostic for the final attempt (if any) is collected by
            // `classify_test_result` and shown in the end-of-run block.
            let final_kind = match &test_result {
                Ok(_) => IndividualTestResultKind::Passed,
                Err(_) => IndividualTestResultKind::Failed,
            };
            self.context.report_test_attempt(
                qualified_test_name,
                attempt,
                final_kind,
                final_attempt_duration,
            );
        }

        RetryOutcome {
            test_result,
            attempt,
            max_attempts,
            was_retried,
        }
    }

    /// Run a test variant (a specific combination of parametrize values and fixtures).
    fn execute_test_variant(&self, py: Python<'_>, variant: TestVariant<'_>) -> bool {
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

        let _ = set_test_name_env(py, &qualified_test_name.to_string());

        // Set snapshot context so `karva.assert_snapshot()` can determine the current test.
        // Use `function_name()` (not `qualified_test_name`) to avoid doubling the module prefix,
        // since `snapshot_path()` already prepends the module name from the file stem.
        let snapshot_test_name =
            full_test_name(py, name.function_name().to_string(), &function_arguments);
        crate::extensions::functions::snapshot::set_snapshot_context(
            test_module_path.to_string(),
            snapshot_test_name,
        );

        let is_async = stmt_function_def.is_async
            && !crate::utils::patch_async_test_function(py, &function).unwrap_or(false);
        let timeout_seconds = tags.timeout_tag().map(TimeoutTag::seconds).or_else(|| {
            self.context
                .settings()
                .test()
                .timeout
                .map(|d| d.as_secs_f64())
        });
        let run_test = || {
            if let Some(seconds) = timeout_seconds {
                return run_test_with_timeout(
                    py,
                    &function,
                    &function_arguments,
                    is_async,
                    seconds,
                );
            }
            let result = if function_arguments.is_empty() {
                function.call0(py)
            } else {
                let py_dict = PyDict::new(py);
                for (key, value) in &function_arguments {
                    py_dict.set_item(key, value.as_ref())?;
                }
                function.call(py, (), Some(&py_dict))
            };
            if is_async {
                result.and_then(|coroutine| run_coroutine(py, coroutine))
            } else {
                result
            }
        };

        let configured_retries = self.context.settings().test().retry;
        let RetryOutcome {
            test_result,
            attempt,
            max_attempts,
            was_retried,
        } = self.run_with_retries(py, &qualified_test_name, configured_retries, run_test);

        let report_ctx = VariantReportCtx {
            name: &name,
            test_module_path: &test_module_path,
            stmt_function_def: &stmt_function_def,
            function_arguments: &function_arguments,
            expect_fail_tag,
        };

        let total_duration = start_time.elapsed();
        self.maybe_register_slow(&qualified_test_name, total_duration);

        let passed = if was_retried {
            let passed_on = attempt;
            // `total_attempts` mirrors nextest: the maximum number of attempts
            // the test was allowed (`retries + 1`), not just the count that
            // ran. This keeps `FLAKY M/T` readable as "passed on attempt M
            // out of an allowed T."
            let total_attempts = max_attempts;
            self.classify_test_result(py, test_result, fixture_call_errors, &report_ctx, |kind| {
                self.context.register_retried_result(
                    &qualified_test_name,
                    &kind,
                    total_duration,
                    passed_on,
                    total_attempts,
                )
            })
        } else {
            self.classify_test_result(py, test_result, fixture_call_errors, &report_ctx, |kind| {
                self.context
                    .register_test_case_result(&qualified_test_name, kind, total_duration)
            })
        };

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

        for dep in fixture.dependencies() {
            match self.run_fixture(py, dep) {
                Ok((value, finalizer)) => {
                    function_arguments.insert(dep.function_name().to_string(), value.clone_ref(py));

                    if let Some(finalizer) = finalizer {
                        self.finalizer_cache.add_finalizer(finalizer);
                    }
                }
                Err(mut err) => {
                    err.dependency_chain.push(FixtureChainEntry {
                        name: fixture.name.function_name().to_string(),
                        source_file: source_file(fixture.name.module_path().path()),
                        stmt_function_def: fixture.stmt_function_def.clone(),
                    });
                    return Err(err);
                }
            }
        }

        let fixture_call_result =
            fixture
                .call(py, &function_arguments)
                .map_err(|err| FixtureCallError {
                    fixture_name: fixture.name.function_name().to_string(),
                    error: err,
                    stmt_function_def: fixture.stmt_function_def.clone(),
                    source_file: source_file(fixture.name.module_path().path()),
                    arguments: function_arguments,
                    dependency_chain: Vec::new(),
                })?;

        let (final_result, finalizer) = get_value_and_finalizer(py, fixture, fixture_call_result)
            .map_err(|err| FixtureCallError {
            fixture_name: fixture.name.function_name().to_string(),
            error: err,
            stmt_function_def: fixture.stmt_function_def.clone(),
            source_file: source_file(fixture.name.module_path().path()),
            arguments: HashMap::new(),
            dependency_chain: Vec::new(),
        })?;

        self.fixture_cache.insert(
            fixture.function_name().to_string(),
            final_result.clone_ref(py),
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
    if fixture.is_generator && fixture.stmt_function_def.is_async {
        // Async generator fixture: call __anext__() and await the coroutine
        let bound = fixture_call_result.bind(py);
        let anext_coroutine = bound.call_method0("__anext__")?;
        let value = run_coroutine(py, anext_coroutine.unbind())?;

        let finalizer = Finalizer {
            fixture_return: fixture_call_result,
            is_async: true,
            scope: fixture.scope(),
            fixture_name: Some(fixture.name.clone()),
            stmt_function_def: Some(fixture.stmt_function_def.clone()),
        };

        Ok((value, Some(finalizer)))
    } else if fixture.is_generator
        && let Ok(mut bound_iterator) = fixture_call_result
            .clone_ref(py)
            .into_bound(py)
            .cast_into::<PyIterator>()
    {
        // Sync generator fixture: call next() to get the yielded value
        match bound_iterator.next() {
            Some(Ok(value)) => {
                let finalizer = Finalizer {
                    fixture_return: bound_iterator.clone().unbind().into_any(),
                    is_async: false,
                    scope: fixture.scope(),
                    fixture_name: Some(fixture.name.clone()),
                    stmt_function_def: Some(fixture.stmt_function_def.clone()),
                };

                Ok((value.unbind(), Some(finalizer)))
            }
            Some(Err(err)) => Err(err),
            None => Err(pyo3::exceptions::PyRuntimeError::new_err(
                "Generator fixture yielded no value",
            )),
        }
    } else {
        Ok((fixture_call_result, None))
    }
}

/// Outcome of driving a test through the configured retry budget.
struct RetryOutcome {
    test_result: PyResult<Py<PyAny>>,
    /// The attempt number on which the test produced its final result.
    attempt: u32,
    /// The maximum number of attempts the test was allowed (`retries + 1`).
    max_attempts: u32,
    /// `true` if at least one retry occurred.
    was_retried: bool,
}

/// Immutable per-variant state threaded into [`PackageRunner::classify_test_result`].
struct VariantReportCtx<'a> {
    name: &'a QualifiedFunctionName,
    test_module_path: &'a camino::Utf8Path,
    stmt_function_def: &'a StmtFunctionDef,
    function_arguments: &'a FixtureArguments,
    expect_fail_tag: Option<ExpectFailTag>,
}

pub struct FixtureCallError {
    pub(crate) fixture_name: String,
    pub(crate) error: PyErr,
    pub(crate) stmt_function_def: Rc<StmtFunctionDef>,
    pub(crate) source_file: SourceFile,
    pub(crate) arguments: FixtureArguments,
    /// The dependency path from the outermost requested fixture down to (but not including)
    /// the fixture that actually failed. Built bottom-up during error propagation.
    pub(crate) dependency_chain: Vec<FixtureChainEntry>,
}

/// An entry in the fixture dependency chain, representing an intermediate fixture
/// between the test and the fixture that actually failed.
pub struct FixtureChainEntry {
    pub(crate) name: String,
    pub(crate) source_file: SourceFile,
    pub(crate) stmt_function_def: Rc<StmtFunctionDef>,
}
