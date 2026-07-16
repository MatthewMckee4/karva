use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use karva_coverage::CoverageSession;
use karva_diagnostic::IndividualTestResultKind;
use karva_metadata::RunIgnoredMode;
use karva_metadata::filter::EvalContext;
use karva_python_semantic::{FunctionKind, QualifiedFunctionName, QualifiedTestName};
use pyo3::prelude::*;
use pyo3::types::PyIterator;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::Context;
use crate::diagnostic::{
    report_fixture_cycle, report_fixture_failure, report_missing_fixtures, report_test_failure,
    report_test_pass_on_expect_failure, report_test_returned_value,
};
use crate::discovery::{DiscoveredModule, DiscoveredPackage};
use crate::extensions::fixtures::{
    Finalizer, FixtureScope, HasFixtures, NormalizedFixture, missing_arguments_from_error,
};
use crate::extensions::tags::expect_fail::ExpectFailTag;
use crate::extensions::tags::skip::{extract_skip_reason, is_skip_exception};
use crate::extensions::tags::timeout::TimeoutTag;
use crate::output_capture::PythonOutputCapture;
use crate::runner::fixture_resolver::RuntimeFixtureResolver;
use crate::runner::test_iterator::{TestVariant, TestVariantIterator};
use crate::runner::{FinalizerCache, FixtureArguments, FixtureCache, FixtureCycleError};
use crate::utils::{
    full_test_name, run_coroutine, run_test_with_timeout, set_attempt_env, set_test_name_env,
    truncate_string,
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

    /// Active coverage session, when coverage is enabled for this worker.
    coverage: Option<&'ctx CoverageSession>,

    /// Running count of failed tests observed during this run.
    ///
    /// Used to enforce `--max-fail=N`: once this counter reaches the
    /// configured budget we stop scheduling new tests.
    failed_count: Cell<u32>,
}

impl<'ctx, 'a> PackageRunner<'ctx, 'a> {
    pub(crate) fn new(context: &'ctx Context<'a>, coverage: Option<&'ctx CoverageSession>) -> Self {
        Self {
            context,
            fixture_cache: FixtureCache::default(),
            finalizer_cache: FinalizerCache::default(),
            coverage,
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
        threshold: Option<std::time::Duration>,
    ) {
        if let Some(threshold) = threshold
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

    fn start_output_capture(&self, py: Python<'_>) -> Option<PythonOutputCapture> {
        if self.context.settings().terminal().show_python_output {
            return None;
        }

        match PythonOutputCapture::start(py) {
            Ok(capture) => Some(capture),
            Err(err) => {
                tracing::warn!("failed to start Python output capture: {err}");
                None
            }
        }
    }

    fn register_captured_output(
        &self,
        py: Python<'_>,
        capture: Option<PythonOutputCapture>,
        test_name: &QualifiedTestName,
        result: &IndividualTestResultKind,
    ) {
        let Some(capture) = capture else {
            return;
        };

        match capture.finish(py) {
            Ok(output) => self.context.register_captured_output(
                test_name,
                result,
                output.stdout,
                output.stderr,
            ),
            Err(err) => tracing::warn!("failed to finish Python output capture: {err}"),
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
        if let Err(error) = self.run_auto_use_fixtures(py, &[], session, FixtureScope::Session) {
            report_fixture_cycle(self.context, error);
            return;
        }

        self.execute_package(py, session, &[]);

        self.clean_up_scope(py, FixtureScope::Session);
    }

    /// Resolve and run auto-use fixtures for `scope`. Resolution cycles are
    /// returned to the caller; execution failures are reported here. The
    /// `current` source is whichever `HasFixtures` provider applies for this
    /// scope (the session package, a module, or a package configuration module).
    fn run_auto_use_fixtures<'b>(
        &self,
        py: Python<'_>,
        parents: &'b [&'b DiscoveredPackage],
        current: &'b (dyn HasFixtures<'b> + 'b),
        scope: FixtureScope,
    ) -> Result<(), FixtureCycleError> {
        let mut resolver = RuntimeFixtureResolver::new(parents, current);
        let auto_use_fixtures = resolver.get_normalized_auto_use_fixtures(py, scope)?;
        let auto_use_errors = self.run_fixtures(py, &auto_use_fixtures);
        for error in auto_use_errors {
            report_fixture_failure(self.context, py, error);
        }

        Ok(())
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
        if let Err(error) = self.run_auto_use_fixtures(py, parents, module, FixtureScope::Module) {
            report_fixture_cycle(self.context, error);
            for test_function in module.test_functions() {
                let test_name = QualifiedTestName::new(test_function.name.clone(), None);
                let test_passed = self.context.register_test_case_result(
                    &test_name,
                    IndividualTestResultKind::Failed,
                    std::time::Duration::ZERO,
                );
                self.record_outcome(test_passed);
                if self.max_fail_reached() {
                    break;
                }
            }
            return false;
        }

        let mut passed = true;

        for test_function in module.test_functions() {
            // Create a new resolver for each test to handle fixture resolution
            let mut test_resolver = RuntimeFixtureResolver::new(parents, module);

            let variants = match TestVariantIterator::new(py, test_function, &mut test_resolver) {
                Ok(variants) => variants,
                Err(error) => {
                    report_fixture_cycle(self.context, error);
                    let test_name = QualifiedTestName::new(test_function.name.clone(), None);
                    let test_passed = self.context.register_test_case_result(
                        &test_name,
                        IndividualTestResultKind::Failed,
                        std::time::Duration::ZERO,
                    );
                    self.record_outcome(test_passed);
                    passed = false;
                    if self.max_fail_reached() {
                        break;
                    }
                    continue;
                }
            };

            // Iterate over all test variants (parametrize combinations × fixture combinations).
            for variant in variants {
                let variant_passed = self.execute_test_variant(py, variant);
                self.record_outcome(variant_passed);
                passed &= variant_passed;

                if self.max_fail_reached() {
                    break;
                }
            }

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
            if let Err(error) =
                self.run_auto_use_fixtures(py, parents, config_module, FixtureScope::Package)
            {
                report_fixture_cycle(self.context, error);
                return false;
            }
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
        test_result: PyResult<TestCallOutcome>,
        fixture_call_errors: Vec<FixtureCallError>,
        ctx: &VariantReportCtx<'_>,
        register: impl FnOnce(IndividualTestResultKind) -> bool,
    ) -> bool {
        let expect_fail = ctx
            .expect_fail_tag
            .as_ref()
            .is_some_and(ExpectFailTag::should_expect_fail);

        let err = match test_result {
            Ok(TestCallOutcome::ReturnedValue(_)) if expect_fail => {
                return register(IndividualTestResultKind::Passed);
            }
            Ok(TestCallOutcome::ReturnedValue(value)) => {
                report_test_returned_value(
                    self.context,
                    ctx.source_file.clone(),
                    ctx.stmt_function_def,
                    &value,
                );
                return register(IndividualTestResultKind::Failed);
            }
            Ok(TestCallOutcome::ReturnedNone) if expect_fail => {
                let reason = ctx.expect_fail_tag.as_ref().and_then(ExpectFailTag::reason);
                report_test_pass_on_expect_failure(
                    self.context,
                    ctx.source_file.clone(),
                    ctx.stmt_function_def,
                    reason,
                );
                return register(IndividualTestResultKind::Failed);
            }
            Ok(TestCallOutcome::ReturnedNone) => return register(IndividualTestResultKind::Passed),
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
                ctx.source_file,
                ctx.stmt_function_def,
                ctx.function_arguments,
                &err,
            );
        } else {
            report_missing_fixtures(
                self.context,
                py,
                ctx.source_file.clone(),
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
        expect_fail: bool,
        mut run_test: impl FnMut() -> PyResult<TestCallOutcome>,
    ) -> RetryOutcome {
        let max_attempts = configured_retries.saturating_add(1);
        let mut run_attempt =
            |attempt| set_attempt_env(py, attempt, max_attempts).and_then(|()| run_test());

        let mut attempt: u32 = 1;
        let mut attempt_start = std::time::Instant::now();
        let mut test_result = run_attempt(attempt);

        let mut retry_count = configured_retries;
        let mut was_retried = false;
        let mut final_attempt_duration = attempt_start.elapsed();

        while retry_count > 0 {
            if !should_retry_result(py, &test_result, expect_fail) {
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
            attempt_start = std::time::Instant::now();
            test_result = run_attempt(attempt);
            final_attempt_duration = attempt_start.elapsed();
        }

        if was_retried {
            // Emit the per-attempt line for the final attempt so output
            // ordering matches nextest:
            //   TRY 1 FAIL ...
            //   TRY 2 PASS ...   (or TRY 2 FAIL for an exhausted retry)
            // The diagnostic for the final attempt (if any) is collected by
            // `classify_test_result` and shown in the end-of-run block.
            let final_kind = attempt_result_kind(py, &test_result);
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
        let source_file = test.source_file.clone();

        if let Some(result) = self.should_skip_variant(&name, &tags) {
            return result;
        }

        let output_capture = self.start_output_capture(py);
        let start_time = std::time::Instant::now();
        let expect_fail_tag = tags.expect_fail_tag();

        let (function_arguments, fixture_call_errors, test_finalizers) = self.setup_test_fixtures(
            py,
            &fixture_dependencies,
            &use_fixture_dependencies,
            &auto_use_fixtures,
            params,
        );

        let fixture_names = fixture_dependencies
            .iter()
            .map(|fixture| fixture.function_name())
            .collect::<Vec<_>>();
        let framework_fixture_names = fixture_dependencies
            .iter()
            .filter(|fixture| fixture.name.module_path().module_name() == "karva._builtins")
            .map(|fixture| fixture.function_name())
            .collect::<Vec<_>>();
        let computed_full_test_name = full_test_name(
            py,
            name.to_string(),
            &function_arguments,
            &stmt_function_def.parameters,
            &framework_fixture_names,
        );

        let qualified_test_name =
            QualifiedTestName::new(name.clone(), Some(computed_full_test_name));

        tracing::debug!("Running test `{}`", qualified_test_name);

        let test_name_env_result = set_test_name_env(py, &qualified_test_name.to_string());

        // Parameter values distinguish snapshot variants, but fixture values can be
        // machine-specific, so snapshot identity includes fixture names only. Use the
        // unqualified function name because `snapshot_path()` prepends the test file stem.
        let snapshot_test_name = full_test_name(
            py,
            name.function_name().to_string(),
            &function_arguments,
            &stmt_function_def.parameters,
            &fixture_names,
        );
        crate::extensions::functions::snapshot::set_snapshot_context(
            test_module_path.to_string(),
            snapshot_test_name,
        );

        let custom_tag_names = tags.custom_tag_names();
        let qualified_name_str = qualified_test_name.to_string();
        let eval_ctx = karva_metadata::filter::EvalContext {
            test_name: &qualified_name_str,
            tags: &custom_tag_names,
        };

        let async_patch_result = if stmt_function_def.is_async {
            crate::utils::patch_async_test_function(py, &function)
        } else {
            Ok(false)
        };
        let is_async = stmt_function_def.is_async && matches!(&async_patch_result, Ok(false));
        let timeout_seconds = tags.timeout_tag().map(TimeoutTag::seconds).or_else(|| {
            self.context
                .settings()
                .timeout_for(&eval_ctx)
                .map(|d| d.as_secs_f64())
        });
        let run_test = || {
            if let Err(err) = &test_name_env_result {
                return Err(err.clone_ref(py));
            }
            if let Err(err) = &async_patch_result {
                return Err(err.clone_ref(py));
            }
            let result = if let Some(seconds) = timeout_seconds {
                run_test_with_timeout(py, &function, &function_arguments, is_async, seconds)
            } else {
                let result = if function_arguments.is_empty() {
                    function.call0(py)
                } else {
                    let py_dict = function_arguments.to_kwargs(py)?;
                    function.call(py, (), Some(&py_dict))
                };
                if is_async {
                    result.and_then(|coroutine| run_coroutine(py, coroutine))
                } else {
                    result
                }
            };

            result.map(|value| reject_non_none_return(py, &value))
        };

        let configured_retries = self.context.settings().retry_for(&eval_ctx);
        let expect_fail = expect_fail_tag
            .as_ref()
            .is_some_and(ExpectFailTag::should_expect_fail);
        self.context.report_test_started(&qualified_test_name);
        if let Some(coverage) = self.coverage {
            coverage.set_current_context(py, Some(&qualified_name_str));
        }
        let RetryOutcome {
            test_result,
            attempt,
            max_attempts,
            was_retried,
        } = self.run_with_retries(
            py,
            &qualified_test_name,
            configured_retries,
            expect_fail,
            run_test,
        );
        self.context.report_test_finished(&qualified_test_name);

        let report_ctx = VariantReportCtx {
            name: &name,
            source_file: &source_file,
            stmt_function_def: &stmt_function_def,
            function_arguments: &function_arguments,
            expect_fail_tag,
        };

        let total_duration = start_time.elapsed();
        self.maybe_register_slow(
            &qualified_test_name,
            total_duration,
            self.context.settings().slow_timeout_for(&eval_ctx),
        );

        let mut final_kind = None;
        let passed = if was_retried {
            let passed_on = attempt;
            // `total_attempts` mirrors nextest: the maximum number of attempts
            // the test was allowed (`retries + 1`), not just the count that
            // ran. This keeps `FLAKY M/T` readable as "passed on attempt M
            // out of an allowed T."
            let total_attempts = max_attempts;
            self.classify_test_result(py, test_result, fixture_call_errors, &report_ctx, |kind| {
                final_kind = Some(kind.clone());
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
                final_kind = Some(kind.clone());
                self.context
                    .register_test_case_result(&qualified_test_name, kind, total_duration)
            })
        };

        for finalizer in test_finalizers.into_iter().rev() {
            finalizer.run(self.context, py);
        }

        self.clean_up_scope(py, FixtureScope::Function);
        match final_kind.as_ref() {
            Some(kind) => {
                self.register_captured_output(py, output_capture, &qualified_test_name, kind);
            }
            None => {
                if let Some(capture) = output_capture
                    && let Err(err) = capture.finish(py)
                {
                    tracing::warn!(
                        "discarded Python output capture for `{qualified_test_name}` after missing result kind: {err}"
                    );
                }
            }
        }
        if let Some(coverage) = self.coverage {
            coverage.set_current_context(py, None);
        }

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

        let mut function_arguments = FixtureArguments::default();

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
                        source_file: fixture.source_file.clone(),
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
                    source_file: fixture.source_file.clone(),
                    arguments: function_arguments,
                    dependency_chain: Vec::new(),
                })?;

        let (final_result, finalizer) = get_value_and_finalizer(py, fixture, fixture_call_result)
            .map_err(|err| FixtureCallError {
            fixture_name: fixture.name.function_name().to_string(),
            error: err,
            stmt_function_def: fixture.stmt_function_def.clone(),
            source_file: fixture.source_file.clone(),
            arguments: FixtureArguments::default(),
            dependency_chain: Vec::new(),
        })?;

        self.fixture_cache.insert(
            fixture.function_name().to_string(),
            final_result.clone_ref(py),
            fixture.scope(),
        );

        let return_finalizer = finalizer.and_then(|f| {
            if f.scope == FixtureScope::Function {
                Some(f)
            } else {
                self.finalizer_cache.add_finalizer(f);
                None
            }
        });

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
            stmt_function_def: Some(fixture.stmt_function_def.clone()),
            source_file: Some(fixture.source_file.clone()),
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
                    stmt_function_def: Some(fixture.stmt_function_def.clone()),
                    source_file: Some(fixture.source_file.clone()),
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

fn reject_non_none_return(py: Python<'_>, value: &Py<PyAny>) -> TestCallOutcome {
    if value.bind(py).is_none() {
        TestCallOutcome::ReturnedNone
    } else {
        TestCallOutcome::ReturnedValue(returned_value_repr(py, value))
    }
}

fn attempt_result_kind(
    py: Python<'_>,
    test_result: &PyResult<TestCallOutcome>,
) -> IndividualTestResultKind {
    match test_result {
        Ok(TestCallOutcome::ReturnedNone) => IndividualTestResultKind::Passed,
        Ok(TestCallOutcome::ReturnedValue(_)) => IndividualTestResultKind::Failed,
        Err(err) if is_skip_exception(py, err) => IndividualTestResultKind::Skipped {
            reason: extract_skip_reason(py, err),
        },
        Err(_) => IndividualTestResultKind::Failed,
    }
}

fn should_retry_result(
    py: Python<'_>,
    test_result: &PyResult<TestCallOutcome>,
    expect_fail: bool,
) -> bool {
    if expect_fail {
        return false;
    }

    match test_result {
        Ok(TestCallOutcome::ReturnedNone) => false,
        Ok(TestCallOutcome::ReturnedValue(_)) => true,
        Err(err) => !is_skip_exception(py, err),
    }
}

fn returned_value_repr(py: Python<'_>, value: &Py<PyAny>) -> String {
    match value.bind(py).repr() {
        Ok(repr) => truncate_string(&repr.to_string()),
        Err(err) => {
            let error = truncate_string(&err.value(py).to_string());
            format!("<repr failed: {error}>")
        }
    }
}

enum TestCallOutcome {
    ReturnedNone,
    ReturnedValue(String),
}

/// Outcome of driving a test through the configured retry budget.
struct RetryOutcome {
    test_result: PyResult<TestCallOutcome>,
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
    source_file: &'a SourceFile,
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
