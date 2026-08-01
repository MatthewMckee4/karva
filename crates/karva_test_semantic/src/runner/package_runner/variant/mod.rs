//! Orchestration and reporting for one concrete test variant.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use karva_diagnostic::{CapturedTestOutput, TestCaseRetry, TestExecutionOutcome};
use karva_metadata::RunIgnoredMode;
use karva_metadata::filter::EvalContext;
use karva_python_semantic::QualifiedTestName;
use pyo3::prelude::*;

use crate::extensions::fixtures::NormalizedFixture;
use crate::extensions::functions::snapshot::SnapshotContext;
use crate::extensions::tags::Tags;
use crate::extensions::tags::expect_fail::ExpectFailTag;
use crate::extensions::tags::fail_slow::FailSlowTag;
use crate::extensions::tags::timeout::TimeoutTag;
use crate::output_capture::PythonOutputCapture;
use crate::runner::fixture_arguments::FixtureArguments;
use crate::runner::test_iterator::TestVariant;
use crate::utils::{
    full_test_name, set_attempt_env, set_fixture_request_context, set_test_name_env,
};

use super::PackageRunner;

mod attempt;

use attempt::{PreparedTestAttempt, TestLifecycleAttempt};

impl PackageRunner<'_, '_> {
    /// Runs one concrete parameter and fixture combination.
    pub(super) fn execute_test_variant(&self, py: Python<'_>, variant: TestVariant<'_>) -> bool {
        VariantRunner::new(self, py, variant).execute()
    }
}

/// Owns all state that remains stable while one test variant retries.
///
/// Keeping variant state separate from [`PackageRunner`] makes retry behavior
/// local: run-wide caches stay on the package runner, while parameter values,
/// fixture selections, tags, and identity stay here.
struct VariantRunner<'runner, 'context, 'settings, 'test, 'py> {
    /// Run-wide owner for fixtures, reporting, settings, and coverage.
    package_runner: &'runner PackageRunner<'context, 'settings>,
    /// Attached Python interpreter used by every attempt.
    py: Python<'py>,
    /// Discovered test definition shared by every variant.
    test: &'test crate::discovery::DiscoveredTestFunction,
    /// Parameter values reused when a retry prepares fresh fixtures.
    params: HashMap<String, Arc<Py<PyAny>>>,
    /// User-defined parameter ID used in the displayed test name.
    id: Option<String>,
    /// Fixtures passed as Python keyword arguments.
    fixture_dependencies: Rc<[Rc<NormalizedFixture>]>,
    /// Fixtures run for side effects but omitted from test arguments.
    use_fixture_dependencies: Rc<[Rc<NormalizedFixture>]>,
    /// Function-scoped auto-use fixtures.
    auto_use_fixtures: Rc<[Rc<NormalizedFixture>]>,
    /// Test, parameter, and fixture tags resolved for this variant.
    tags: Tags,
    /// Module path used to build stable snapshot identity.
    module_path: camino::Utf8PathBuf,
}

impl<'runner, 'context, 'settings, 'test, 'py>
    VariantRunner<'runner, 'context, 'settings, 'test, 'py>
{
    /// Captures raw variant inputs before any fixture setup begins.
    fn new(
        package_runner: &'runner PackageRunner<'context, 'settings>,
        py: Python<'py>,
        variant: TestVariant<'test>,
    ) -> Self {
        let tags = variant.resolved_tags();
        let module_path = variant.module_path().clone();
        let TestVariant {
            test,
            params,
            id,
            fixture_dependencies,
            use_fixture_dependencies,
            auto_use_fixtures,
            tags: _,
        } = variant;

        Self {
            package_runner,
            py,
            test,
            params,
            id,
            fixture_dependencies,
            use_fixture_dependencies,
            auto_use_fixtures,
            tags,
            module_path,
        }
    }

    /// Executes setup, all required attempts, teardown, and final reporting.
    fn execute(mut self) -> bool {
        if let Some(result) = self.should_skip() {
            return result;
        }

        let retry_params = self.params.clone();
        let first_params = std::mem::take(&mut self.params);
        let marker_names = self.tags.custom_tag_names();
        let request_context_result =
            set_fixture_request_context(self.py, self.test.name.function_name(), &marker_names);
        let first_attempt = self.prepare_attempt(first_params, self.start_output_capture());
        let settings = self.settings(&first_attempt.fixtures.function_arguments);
        let function = self.test.py_function.clone_ref(self.py);
        let test_name_env_result = request_context_result
            .and_then(|()| set_test_name_env(self.py, &settings.qualified_test_name.to_string()));

        tracing::debug!("Running test `{}`", settings.qualified_test_name);
        self.package_runner
            .context
            .report_test_started(&settings.qualified_test_name);
        self.set_coverage_context(Some(&settings.qualified_name));

        let mut attempt_number = 1;
        let mut prepared_attempt = Some(first_attempt);
        let mut prior_attempts = Vec::new();

        let final_attempt = loop {
            let attempt_env_result =
                set_attempt_env(self.py, attempt_number, settings.max_attempts);
            let prepared = prepared_attempt
                .take()
                .unwrap_or_else(|| self.prepare_retry(retry_params.clone(), &settings));
            let attempt = self.execute_attempt(
                &settings,
                &function,
                &test_name_env_result,
                attempt_env_result,
                prepared,
                attempt_number,
            );

            if attempt.retryable && attempt_number < settings.max_attempts {
                self.package_runner.context.report_test_attempt(
                    &settings.qualified_test_name,
                    attempt_number,
                    attempt.lifecycle.outcome.result_kind(),
                    attempt.lifecycle.duration,
                );
                prior_attempts.push(attempt.lifecycle);
                tracing::debug!("Retrying test `{}`", settings.qualified_test_name);
                attempt_number += 1;
            } else {
                break attempt.lifecycle;
            }
        };

        self.finish(&settings, prior_attempts, final_attempt)
    }

    /// Prepares fixture values and records setup duration for one attempt.
    fn prepare_attempt(
        &self,
        params: HashMap<String, Arc<Py<PyAny>>>,
        output_capture: Option<PythonOutputCapture>,
    ) -> PreparedTestAttempt {
        let setup_start = Instant::now();
        let fixtures = self.package_runner.prepare_test_fixtures(
            self.py,
            &self.fixture_dependencies,
            &self.use_fixture_dependencies,
            &self.auto_use_fixtures,
            params,
        );
        PreparedTestAttempt {
            fixtures,
            setup_duration: setup_start.elapsed(),
            output_capture,
        }
    }

    /// Prepares a retry while ensuring coverage excludes fixture setup.
    fn prepare_retry(
        &self,
        params: HashMap<String, Arc<Py<PyAny>>>,
        settings: &VariantSettings,
    ) -> PreparedTestAttempt {
        self.set_coverage_context(None);
        let prepared = self.prepare_attempt(params, self.start_output_capture());
        self.set_coverage_context(Some(&settings.qualified_name));
        prepared
    }

    /// Derives identity and execution policy after first fixture setup.
    fn settings(&self, function_arguments: &FixtureArguments) -> VariantSettings {
        let name = &self.test.name;
        let fixture_names = self
            .fixture_dependencies
            .iter()
            .map(|fixture| fixture.function_name())
            .collect::<Vec<_>>();
        let framework_fixture_names = self
            .fixture_dependencies
            .iter()
            .filter(|fixture| fixture.name.module_path().module_name() == "karva._builtins")
            .map(|fixture| fixture.function_name())
            .collect::<Vec<_>>();
        let full_name = if let Some(id) = &self.id {
            format!("{name}({id})")
        } else {
            full_test_name(
                self.py,
                name.to_string(),
                function_arguments,
                &self.test.stmt_function_def.parameters,
                &framework_fixture_names,
            )
        };
        let qualified_test_name = QualifiedTestName::new(name.clone(), Some(full_name));
        let qualified_name = qualified_test_name.to_string();
        let custom_tag_names = self.tags.custom_tag_names();
        let evaluation_context = EvalContext {
            test_name: &qualified_name,
            tags: &custom_tag_names,
        };
        let fail_slow_budget = self
            .tags
            .fail_slow_tag()
            .map(FailSlowTag::seconds)
            .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
            .or_else(|| {
                self.package_runner
                    .context
                    .settings()
                    .fail_slow_for(&evaluation_context)
            });
        let slow_timeout = self
            .package_runner
            .context
            .settings()
            .slow_timeout_for(&evaluation_context);
        let timeout_seconds = self
            .tags
            .timeout_tag()
            .map(TimeoutTag::seconds)
            .or_else(|| {
                self.package_runner
                    .context
                    .settings()
                    .timeout_for(&evaluation_context)
                    .map(|duration| duration.as_secs_f64())
            });
        let max_attempts = self
            .package_runner
            .context
            .settings()
            .retry_for(&evaluation_context)
            .saturating_add(1);
        let expect_fail_tag = self.tags.expect_fail_tag();
        let expect_fail = expect_fail_tag
            .as_ref()
            .is_some_and(ExpectFailTag::should_expect_fail);
        let async_patch_result = if self.test.stmt_function_def.is_async {
            crate::utils::patch_async_test_function(self.py, &self.test.py_function)
        } else {
            Ok(false)
        };
        let is_async =
            self.test.stmt_function_def.is_async && matches!(&async_patch_result, Ok(false));
        let snapshot_context = SnapshotContext::new(
            self.module_path.to_string(),
            full_test_name(
                self.py,
                name.function_name().to_string(),
                function_arguments,
                &self.test.stmt_function_def.parameters,
                &fixture_names,
            ),
        );

        VariantSettings {
            qualified_test_name,
            qualified_name,
            snapshot_context,
            expect_fail_tag,
            expect_fail,
            async_patch_result,
            is_async,
            timeout_seconds,
            fail_slow_budget,
            slow_timeout,
            max_attempts,
        }
    }

    /// Registers final outcome after all retries and returns pass/fail status.
    fn finish(
        &self,
        settings: &VariantSettings,
        prior_attempts: Vec<TestLifecycleAttempt>,
        final_attempt: TestLifecycleAttempt,
    ) -> bool {
        self.set_coverage_context(None);
        let captured_output = combine_captured_output(
            prior_attempts
                .iter()
                .chain(std::iter::once(&final_attempt))
                .filter_map(|attempt| attempt.captured_output.as_ref()),
        );
        let total_duration = prior_attempts
            .iter()
            .map(|attempt| attempt.duration)
            .sum::<Duration>()
            .saturating_add(final_attempt.duration);

        if !prior_attempts.is_empty() {
            self.package_runner.context.report_test_attempt(
                &settings.qualified_test_name,
                final_attempt.attempt,
                final_attempt.outcome.result_kind(),
                final_attempt.duration,
            );
        }
        if settings
            .slow_timeout
            .is_some_and(|threshold| total_duration > threshold)
        {
            self.package_runner
                .context
                .register_slow_test(&settings.qualified_test_name, total_duration);
        }
        self.package_runner
            .context
            .report_test_finished(&settings.qualified_test_name);

        if prior_attempts.is_empty() {
            self.package_runner.context.register_test_case_result(
                &settings.qualified_test_name,
                final_attempt.outcome,
                total_duration,
                captured_output,
            )
        } else {
            let final_attempt_number = final_attempt.attempt;
            let outcome = final_attempt.outcome.clone();
            let mut execution_attempts = prior_attempts
                .into_iter()
                .map(TestLifecycleAttempt::into_execution_attempt)
                .collect::<Vec<_>>();
            execution_attempts.push(final_attempt.into_execution_attempt());
            self.package_runner.context.register_retried_result(
                &settings.qualified_test_name,
                outcome,
                total_duration,
                TestCaseRetry::new(final_attempt_number, settings.max_attempts),
                captured_output,
                execution_attempts,
            )
        }
    }

    /// Returns a registered skip result when filters or skip policy exclude this variant.
    fn should_skip(&self) -> Option<bool> {
        let filter = &self.package_runner.context.settings().test().filter;
        let run_ignored = self.package_runner.context.settings().test().run_ignored;

        if !filter.is_empty() {
            let qualified = QualifiedTestName::new(self.test.name.clone(), None);
            let display_name = qualified.to_string();
            let custom_names = self.tags.custom_tag_names();
            let context = EvalContext {
                test_name: &display_name,
                tags: &custom_names,
            };
            if !filter.matches(&context) {
                return Some(self.package_runner.context.register_test_case_result(
                    &qualified,
                    TestExecutionOutcome::Skipped { reason: None },
                    Duration::ZERO,
                    None,
                ));
            }
        }

        let skipped = match run_ignored {
            RunIgnoredMode::Default => self.tags.should_skip(),
            RunIgnoredMode::Only => {
                let (should_skip, _) = self.tags.should_skip();
                (!should_skip, None)
            }
            RunIgnoredMode::All => (false, None),
        };
        let (true, reason) = skipped else {
            return None;
        };
        let qualified = QualifiedTestName::new(self.test.name.clone(), None);
        Some(self.package_runner.context.register_test_case_result(
            &qualified,
            TestExecutionOutcome::Skipped { reason },
            Duration::ZERO,
            None,
        ))
    }

    /// Starts best-effort Python output capture when terminal output is hidden.
    fn start_output_capture(&self) -> Option<PythonOutputCapture> {
        if self
            .package_runner
            .context
            .settings()
            .terminal()
            .show_python_output
        {
            return None;
        }

        match PythonOutputCapture::start(self.py) {
            Ok(capture) => Some(capture),
            Err(error) => {
                tracing::warn!("failed to start Python output capture: {error}");
                None
            }
        }
    }

    /// Updates coverage context when coverage is active for this worker.
    fn set_coverage_context(&self, context: Option<&str>) {
        if let Some(coverage) = self.package_runner.coverage {
            coverage.set_current_context(self.py, context);
        }
    }
}

/// Derived identity and execution policy shared by every retry attempt.
struct VariantSettings {
    /// User-visible test name including parameter and fixture identity.
    qualified_test_name: QualifiedTestName,
    /// Cached string form used by coverage.
    qualified_name: String,
    /// Snapshot identity restored before each attempt.
    snapshot_context: SnapshotContext,
    /// Expected-failure tag needed for result classification.
    expect_fail_tag: Option<ExpectFailTag>,
    /// Cached expected-failure decision used by retry policy.
    expect_fail: bool,
    /// Result of patching pytest-style async wrappers.
    async_patch_result: PyResult<bool>,
    /// Whether Karva must await the returned coroutine.
    is_async: bool,
    /// Per-call timeout in seconds, when configured.
    timeout_seconds: Option<f64>,
    /// Full-lifecycle failure budget, when configured.
    fail_slow_budget: Option<Duration>,
    /// Total variant duration threshold for slow reporting.
    slow_timeout: Option<Duration>,
    /// Total attempts, including the initial call.
    max_attempts: u32,
}

/// Finishes best-effort Python output capture.
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
        Err(error) => {
            tracing::warn!("failed to finish Python output capture: {error}");
            None
        }
    }
}

/// Combines attempt output for existing terminal and `JUnit` consumers.
fn combine_captured_output<'a>(
    outputs: impl Iterator<Item = &'a CapturedTestOutput>,
) -> Option<CapturedTestOutput> {
    let mut stdout = String::new();
    let mut stderr = String::new();
    for output in outputs {
        stdout.push_str(output.stdout());
        stderr.push_str(output.stderr());
    }
    let output = CapturedTestOutput::new(stdout, stderr);
    (!output.is_empty()).then_some(output)
}
