use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use karva_coverage::CoverageSession;
use karva_diagnostic::{
    CapturedTestOutput, IndividualTestResultKind, TestCaseRetry, TestExecutionAttempt,
    TestExecutionOutcome,
};
use karva_metadata::RunIgnoredMode;
use karva_metadata::filter::EvalContext;
use karva_python_semantic::{FunctionKind, QualifiedFunctionName, QualifiedTestName};
use pyo3::prelude::*;
use pyo3::types::PyIterator;
use ruff_db::diagnostic::Diagnostic;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::Context;
use crate::diagnostic::{
    fixture_failure_diagnostic, fixture_resolution_diagnostic, invalid_parametrize_diagnostic,
    missing_fixtures_diagnostic, test_failure_diagnostic, test_pass_on_expect_failure_diagnostic,
    test_returned_value_diagnostic,
};
use crate::discovery::{DiscoveredModule, DiscoveredPackage, DiscoveredTestFunction};
use crate::extensions::fixtures::{
    Finalizer, FixtureScope, HasFixtures, NormalizedFixture, missing_arguments_from_error,
};
use crate::extensions::functions::snapshot::{SnapshotContext, set_snapshot_context};
use crate::extensions::tags::expect_fail::ExpectFailTag;
use crate::extensions::tags::skip::{extract_skip_reason, is_skip_exception};
use crate::extensions::tags::timeout::TimeoutTag;
use crate::output_capture::PythonOutputCapture;
use crate::runner::fixture_resolver::RuntimeFixtureResolver;
use crate::runner::test_iterator::{TestVariant, TestVariantIterator};
use crate::runner::{FinalizerCache, FixtureArguments, FixtureCache};
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

    fn register_error_test(
        &self,
        test: &DiscoveredTestFunction,
        diagnostic: Diagnostic,
        related: Vec<Diagnostic>,
    ) {
        self.context.register_test_case_result(
            &QualifiedTestName::new(test.name.clone(), None),
            TestExecutionOutcome::error(diagnostic).with_related(related),
            std::time::Duration::ZERO,
            None,
        );
        self.record_outcome(false);
    }

    fn register_error_module_tests(
        &self,
        module: &DiscoveredModule,
        diagnostic: &Diagnostic,
        related: &[Diagnostic],
    ) {
        for test in module.test_functions() {
            self.register_error_test(test, diagnostic.clone(), related.to_vec());
            if self.max_fail_reached() {
                return;
            }
        }
    }

    fn register_error_package_tests(
        &self,
        package: &DiscoveredPackage,
        diagnostic: &Diagnostic,
        related: &[Diagnostic],
    ) {
        for module in package.modules().values() {
            self.register_error_module_tests(module, diagnostic, related);
            if self.max_fail_reached() {
                return;
            }
        }
        for child_package in package.packages().values() {
            self.register_error_package_tests(child_package, diagnostic, related);
            if self.max_fail_reached() {
                return;
            }
        }
    }

    fn validate_parametrization(&self, package: &DiscoveredPackage) -> bool {
        let mut valid = true;

        for module in package.modules().values() {
            for test_function in module.test_functions() {
                if let Err(error) = test_function
                    .tags
                    .validate_parametrize(&test_function.stmt_function_def)
                {
                    let diagnostic = invalid_parametrize_diagnostic(
                        test_function.source_file.clone(),
                        &test_function.stmt_function_def,
                        &error,
                    );
                    self.register_error_test(test_function, diagnostic, Vec::new());
                    valid = false;
                    if self.max_fail_reached() {
                        return false;
                    }
                }
            }
        }

        for child_package in package.packages().values() {
            valid &= self.validate_parametrization(child_package);
            if self.max_fail_reached() {
                return false;
            }
        }

        valid
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

    fn finish_output_capture(
        py: Python<'_>,
        capture: Option<PythonOutputCapture>,
    ) -> Option<CapturedTestOutput> {
        let capture = capture?;

        match capture.finish(py) {
            Ok(output) => {
                let output = CapturedTestOutput::new(output.stdout, output.stderr);
                (!output.is_empty()).then_some(output)
            }
            Err(err) => {
                tracing::warn!("failed to finish Python output capture: {err}");
                None
            }
        }
    }

    /// Executes all tests in a package.
    ///
    /// The main entrypoint for actual test execution.
    pub(crate) fn execute(&self, py: Python<'_>, session: &DiscoveredPackage) {
        if !self.validate_parametrization(session) {
            return;
        }

        // Resolve session-scoped auto-use fixtures using the session package
        // itself as the `HasFixtures` source so that the walk includes both
        // the user conftest at the session root and the framework module. No
        // `if let Some(...)` gate: the session always exists, and if neither
        // slot contributes any autouse fixtures the walk returns an empty vec.
        if let Err(mut diagnostics) =
            self.run_auto_use_fixtures(py, &[], session, FixtureScope::Session)
        {
            let diagnostic = diagnostics.remove(0);
            self.register_error_package_tests(session, &diagnostic, &diagnostics);
            return;
        }

        self.execute_package(py, session, &[]);

        self.report_scope_cleanup(py, FixtureScope::Session);
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
    ) -> Result<(), Vec<Diagnostic>> {
        let mut resolver = RuntimeFixtureResolver::new(parents, current);
        let auto_use_fixtures = resolver
            .get_normalized_auto_use_fixtures(py, scope)
            .map_err(|error| vec![fixture_resolution_diagnostic(error)])?;
        let auto_use_errors = self.run_fixtures(py, &auto_use_fixtures);
        if auto_use_errors.is_empty() {
            Ok(())
        } else {
            Err(auto_use_errors
                .into_iter()
                .map(|error| fixture_failure_diagnostic(py, error))
                .collect())
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
        if let Err(mut diagnostics) =
            self.run_auto_use_fixtures(py, parents, module, FixtureScope::Module)
        {
            let diagnostic = diagnostics.remove(0);
            self.register_error_module_tests(module, &diagnostic, &diagnostics);
            return false;
        }

        let mut passed = true;

        for test_function in module.test_functions() {
            // Create a new resolver for each test to handle fixture resolution
            let mut test_resolver = RuntimeFixtureResolver::new(parents, module);

            let variants = match TestVariantIterator::new(py, test_function, &mut test_resolver) {
                Ok(variants) => variants,
                Err(error) => {
                    let diagnostic = fixture_resolution_diagnostic(error);
                    self.register_error_test(test_function, diagnostic, Vec::new());
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

        self.report_scope_cleanup(py, FixtureScope::Module);

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
            if let Err(mut diagnostics) =
                self.run_auto_use_fixtures(py, parents, config_module, FixtureScope::Package)
            {
                let diagnostic = diagnostics.remove(0);
                self.register_error_package_tests(package, &diagnostic, &diagnostics);
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

        self.report_scope_cleanup(py, FixtureScope::Package);

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
                    TestExecutionOutcome::Skipped { reason: None },
                    std::time::Duration::ZERO,
                    None,
                ));
            }
        }

        match run_ignored {
            RunIgnoredMode::Default => {
                if let (true, reason) = tags.should_skip() {
                    return Some(self.context.register_test_case_result(
                        &QualifiedTestName::new(name.clone(), None),
                        TestExecutionOutcome::Skipped { reason },
                        std::time::Duration::ZERO,
                        None,
                    ));
                }
            }
            RunIgnoredMode::Only => {
                // Skip tests whose skip condition is not active; only tests
                // that would actually be skipped in a normal run are included.
                if let (false, _) = tags.should_skip() {
                    return Some(self.context.register_test_case_result(
                        &QualifiedTestName::new(name.clone(), None),
                        TestExecutionOutcome::Skipped { reason: None },
                        std::time::Duration::ZERO,
                        None,
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

    /// Classify a test result, handling `expect_fail` logic and diagnostics.
    fn classify_test_result(
        py: Python<'_>,
        test_result: PyResult<TestCallOutcome>,
        ctx: &VariantReportCtx<'_>,
    ) -> TestExecutionOutcome {
        let expect_fail = ctx
            .expect_fail_tag
            .as_ref()
            .is_some_and(ExpectFailTag::should_expect_fail);

        let err = match test_result {
            Ok(TestCallOutcome::ReturnedValue(_)) if expect_fail => {
                return TestExecutionOutcome::Passed;
            }
            Ok(TestCallOutcome::ReturnedValue(value)) => {
                let diagnostic = test_returned_value_diagnostic(
                    ctx.source_file.clone(),
                    ctx.stmt_function_def,
                    &value,
                );
                return TestExecutionOutcome::failed(diagnostic);
            }
            Ok(TestCallOutcome::ReturnedNone) if expect_fail => {
                let reason = ctx.expect_fail_tag.as_ref().and_then(ExpectFailTag::reason);
                let diagnostic = test_pass_on_expect_failure_diagnostic(
                    ctx.source_file.clone(),
                    ctx.stmt_function_def,
                    reason,
                );
                return TestExecutionOutcome::failed(diagnostic);
            }
            Ok(TestCallOutcome::ReturnedNone) => return TestExecutionOutcome::Passed,
            Err(err) => err,
        };

        if is_skip_exception(py, &err) {
            return TestExecutionOutcome::Skipped {
                reason: extract_skip_reason(py, &err),
            };
        }

        if expect_fail {
            return TestExecutionOutcome::Passed;
        }

        let missing_args = missing_arguments_from_error(ctx.name.function_name(), &err.to_string());

        let diagnostic = if missing_args.is_empty() {
            test_failure_diagnostic(
                py,
                ctx.source_file,
                ctx.stmt_function_def,
                ctx.function_arguments,
                &err,
            )
        } else {
            missing_fixtures_diagnostic(
                ctx.source_file.clone(),
                ctx.stmt_function_def,
                &missing_args,
                FunctionKind::Test,
            )
        };

        TestExecutionOutcome::failed(diagnostic)
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
        let mut run_attempt = |attempt| {
            let start = std::time::Instant::now();
            let result = set_attempt_env(py, attempt, max_attempts).and_then(|()| run_test());
            TestCallAttempt {
                attempt,
                result,
                duration: start.elapsed(),
            }
        };
        let mut attempt: u32 = 1;
        let mut current_attempt = run_attempt(attempt);
        let mut retry_count = configured_retries;
        let mut was_retried = false;
        let mut attempts = Vec::new();

        while retry_count > 0 {
            if !should_retry_result(py, &current_attempt.result, expect_fail) {
                break;
            }
            self.context.report_test_attempt(
                qualified_test_name,
                attempt,
                IndividualTestResultKind::Failed,
                current_attempt.duration,
            );
            attempts.push(current_attempt);
            was_retried = true;

            tracing::debug!("Retrying test `{}`", qualified_test_name);
            retry_count -= 1;
            attempt += 1;
            current_attempt = run_attempt(attempt);
        }

        if was_retried {
            // Emit the per-attempt line for the final attempt so output
            // ordering matches nextest:
            //   TRY 1 FAIL ...
            //   TRY 2 PASS ...   (or TRY 2 FAIL for an exhausted retry)
            // The diagnostic for the final attempt (if any) is collected by
            // `classify_test_result` and shown in the end-of-run block.
            let final_kind = attempt_result_kind(py, &current_attempt.result);
            self.context.report_test_attempt(
                qualified_test_name,
                attempt,
                final_kind,
                current_attempt.duration,
            );
        }
        attempts.push(current_attempt);

        RetryOutcome {
            attempt,
            max_attempts,
            was_retried,
            attempts,
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
        let custom_tag_names = tags.custom_tag_names();
        let qualified_name_str = qualified_test_name.to_string();
        let eval_ctx = karva_metadata::filter::EvalContext {
            test_name: &qualified_name_str,
            tags: &custom_tag_names,
        };

        tracing::debug!("Running test `{}`", qualified_test_name);

        if !fixture_call_errors.is_empty() {
            let mut diagnostics = fixture_call_errors
                .into_iter()
                .map(|error| fixture_failure_diagnostic(py, error))
                .collect::<Vec<_>>();
            diagnostics.extend(
                test_finalizers
                    .into_iter()
                    .rev()
                    .filter_map(|finalizer| finalizer.run(py)),
            );
            diagnostics.extend(self.clean_up_scope(py, FixtureScope::Function));
            let captured_output = Self::finish_output_capture(py, output_capture);
            let duration = start_time.elapsed();
            self.maybe_register_slow(
                &qualified_test_name,
                duration,
                self.context.settings().slow_timeout_for(&eval_ctx),
            );
            let diagnostic = diagnostics.remove(0);
            return self.context.register_test_case_result(
                &qualified_test_name,
                TestExecutionOutcome::error(diagnostic).with_related(diagnostics),
                duration,
                captured_output,
            );
        }

        let test_name_env_result = set_test_name_env(py, &qualified_test_name.to_string());

        // Parameter values distinguish snapshot variants, but fixture values can be
        // machine-specific, so snapshot identity includes fixture names only. Use the
        // unqualified function name because `snapshot_path()` prepends the test file stem.
        let snapshot_context = SnapshotContext::new(
            test_module_path.to_string(),
            full_test_name(
                py,
                name.function_name().to_string(),
                &function_arguments,
                &stmt_function_def.parameters,
                &fixture_names,
            ),
        );

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
            set_snapshot_context(snapshot_context.clone());
            if let Err(err) = &test_name_env_result {
                return Err(err.clone_ref(py));
            }
            if let Err(err) = &async_patch_result {
                return Err(err.clone_ref(py));
            }
            let result = if let Some(seconds) = timeout_seconds {
                run_test_with_timeout(
                    py,
                    &function,
                    &function_arguments,
                    is_async,
                    seconds,
                    &snapshot_context,
                )
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
            attempt,
            max_attempts,
            was_retried,
            attempts,
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

        let execution_attempts = attempts
            .into_iter()
            .map(|attempt| {
                TestExecutionAttempt::new(
                    attempt.attempt,
                    Self::classify_test_result(py, attempt.result, &report_ctx),
                    attempt.duration,
                )
            })
            .collect::<Vec<_>>();
        let Some(final_attempt) = execution_attempts.last() else {
            return false;
        };
        let outcome = final_attempt.outcome().clone();

        let mut finalizer_diagnostics = test_finalizers
            .into_iter()
            .rev()
            .filter_map(|finalizer| finalizer.run(py))
            .collect::<Vec<_>>();
        finalizer_diagnostics.extend(self.clean_up_scope(py, FixtureScope::Function));
        let outcome = attach_finalizer_diagnostics(outcome, finalizer_diagnostics);
        let captured_output = Self::finish_output_capture(py, output_capture);
        if let Some(coverage) = self.coverage {
            coverage.set_current_context(py, None);
        }

        if was_retried {
            self.context.register_retried_result(
                &qualified_test_name,
                outcome,
                total_duration,
                TestCaseRetry::new(attempt, max_attempts),
                captured_output,
                execution_attempts,
            )
        } else {
            self.context.register_test_case_result(
                &qualified_test_name,
                outcome,
                total_duration,
                captured_output,
            )
        }
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
    fn clean_up_scope(&self, py: Python, scope: FixtureScope) -> Vec<Diagnostic> {
        let diagnostics = self.finalizer_cache.run_and_clear_scope(py, scope);
        self.fixture_cache.clear_fixtures(scope);
        diagnostics
    }

    fn report_scope_cleanup(&self, py: Python, scope: FixtureScope) {
        for diagnostic in self.clean_up_scope(py, scope) {
            self.context.add_run_diagnostic(diagnostic);
        }
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

fn attach_finalizer_diagnostics(
    outcome: TestExecutionOutcome,
    mut diagnostics: Vec<Diagnostic>,
) -> TestExecutionOutcome {
    if diagnostics.is_empty() {
        return outcome;
    }

    match outcome {
        TestExecutionOutcome::Failed {
            diagnostic,
            mut related,
        } => {
            related.append(&mut diagnostics);
            TestExecutionOutcome::Failed {
                diagnostic,
                related,
            }
        }
        TestExecutionOutcome::Error {
            diagnostic,
            mut related,
        } => {
            related.append(&mut diagnostics);
            TestExecutionOutcome::Error {
                diagnostic,
                related,
            }
        }
        TestExecutionOutcome::Passed | TestExecutionOutcome::Skipped { .. } => {
            let diagnostic = diagnostics.remove(0);
            TestExecutionOutcome::error(diagnostic).with_related(diagnostics)
        }
    }
}

enum TestCallOutcome {
    ReturnedNone,
    ReturnedValue(String),
}

/// Outcome of driving a test through the configured retry budget.
struct RetryOutcome {
    /// The attempt number on which the test produced its final result.
    attempt: u32,
    /// The maximum number of attempts the test was allowed (`retries + 1`).
    max_attempts: u32,
    /// `true` if at least one retry occurred.
    was_retried: bool,
    attempts: Vec<TestCallAttempt>,
}

struct TestCallAttempt {
    attempt: u32,
    result: PyResult<TestCallOutcome>,
    duration: std::time::Duration,
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
