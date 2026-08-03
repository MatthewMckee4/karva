//! Orchestration and reporting for one concrete test variant.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use karva_coverage::CoveragePhase;
use karva_diagnostic::{CapturedTestOutput, TestCaseRetry, TestExecutionOutcome};
use karva_metadata::filter::EvalContext;
use karva_metadata::{FlakyResult, JunitFlakyFailStatus, RunIgnoredMode};
use karva_python_semantic::QualifiedTestName;
use pyo3::prelude::*;

use crate::extensions::fixtures::{FixtureId, FixturePlan, FixtureScope, NormalizedFixture};
use crate::extensions::functions::snapshot::SnapshotContext;
use crate::extensions::tags::RuntimeTags;
use crate::extensions::tags::expect_fail::ExpectFailTag;
use crate::extensions::tags::fail_slow::FailSlowTag;
use crate::extensions::tags::timeout::TimeoutTag;
use crate::output_capture::PythonOutputCapture;
use crate::runner::fixture_arguments::FixtureArguments;
use crate::runner::test_iterator::{
    FixtureParameter, FixtureVariantMetadata, ParameterIdentity, TestVariant,
};
use crate::utils::{set_attempt_env, set_test_name_env, test_parameters};

use super::PackageRunner;
use super::fixture::TestFixtureInputs;

mod attempt;

use attempt::{PreparedTestAttempt, TestLifecycleAttempt};

impl PackageRunner<'_, '_> {
    /// Runs one concrete parameter and fixture combination.
    pub(super) fn execute_test_variant(
        &mut self,
        py: Python<'_>,
        variant: TestVariant<'_>,
    ) -> bool {
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
    package_runner: &'runner mut PackageRunner<'context, 'settings>,
    /// Attached Python interpreter used by every attempt.
    py: Python<'py>,
    /// Discovered test definition shared by every variant.
    test: &'test crate::discovery::DiscoveredTestFunction,
    /// Parameter values reused when a retry prepares fresh fixtures.
    params: HashMap<String, Arc<Py<PyAny>>>,
    /// Parameter values selected for fixture request objects.
    fixture_params: Option<Rc<HashMap<String, FixtureParameter>>>,
    /// Explicit scopes assigned to direct parametrization values.
    parameter_scopes: Option<Rc<HashMap<String, FixtureScope>>>,
    /// Display and collection identity for a parametrized test.
    identity: Option<Box<ParameterIdentity>>,
    /// Compiled fixture arena for this test.
    fixture_plan: Rc<FixturePlan>,
    /// Fixtures passed as Python keyword arguments.
    fixture_dependencies: Rc<[FixtureId]>,
    /// Fixtures run for side effects but omitted from test arguments.
    use_fixture_dependencies: Rc<[FixtureId]>,
    /// Function-scoped auto-use fixtures.
    auto_use_fixtures: Rc<[FixtureId]>,
    /// Test, parameter, and fixture tags resolved for this variant.
    tags: RuntimeTags,
    /// Module path used to build stable snapshot identity.
    module_path: camino::Utf8PathBuf,
}

impl<'runner, 'context, 'settings, 'test, 'py>
    VariantRunner<'runner, 'context, 'settings, 'test, 'py>
{
    /// Captures raw variant inputs before any fixture setup begins.
    fn new(
        package_runner: &'runner mut PackageRunner<'context, 'settings>,
        py: Python<'py>,
        variant: TestVariant<'test>,
    ) -> Self {
        let module_path = variant.module_path().clone();
        let TestVariant {
            test,
            params,
            fixture_metadata,
            identity,
            fixture_plan,
            fixture_dependencies,
            use_fixture_dependencies,
            auto_use_fixtures,
            tags,
        } = variant;

        let (fixture_params, parameter_scopes) = fixture_metadata.map_or_else(
            || (None, None),
            |metadata| {
                let FixtureVariantMetadata { parameters, scoped } = *metadata;
                let parameter_scopes = (!scoped.is_empty()).then(|| {
                    Rc::new(
                        scoped
                            .into_iter()
                            .map(|(name, parameter)| (name, parameter.scope))
                            .collect(),
                    )
                });
                (parameters, parameter_scopes)
            },
        );

        Self {
            package_runner,
            py,
            test,
            params,
            fixture_params,
            parameter_scopes,
            identity,
            fixture_plan,
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
        self.begin_pending_coverage_setup();
        let first_attempt = self.prepare_attempt(first_params, self.start_output_capture());
        let settings = self.settings(
            &first_attempt.fixtures.function_arguments,
            &first_attempt.fixtures.request_tags,
        );
        self.resolve_pending_coverage_setup(&settings.qualified_name);
        let function = self.test.py_function.clone_ref(self.py);
        let test_name_env_result =
            set_test_name_env(self.py, &settings.qualified_test_name.to_string());

        tracing::debug!("Running test `{}`", settings.qualified_test_name);
        self.package_runner
            .context
            .report_test_started(&settings.qualified_test_name);

        let mut attempt_number = 1;
        let mut prepared_attempt = Some(first_attempt);
        let mut prior_attempts = Vec::new();

        let final_attempt = loop {
            let attempt_env_result =
                set_attempt_env(self.py, attempt_number, settings.max_attempts);
            let prepared = prepared_attempt
                .take()
                .unwrap_or_else(|| self.prepare_retry(retry_params.clone(), &settings));
            self.set_coverage_context(&settings.qualified_name, CoveragePhase::Run);
            let attempt = self.execute_attempt(
                &settings,
                &function,
                &test_name_env_result,
                attempt_env_result,
                prepared,
                attempt_number,
            );

            if attempt.retryable && attempt_number < settings.max_attempts {
                self.package_runner.state.report_test_attempt(
                    self.package_runner.context,
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
            self.test,
            TestFixtureInputs {
                fixture_plan: &self.fixture_plan,
                fixture_dependencies: &self.fixture_dependencies,
                use_fixture_dependencies: &self.use_fixture_dependencies,
                auto_use_fixtures: &self.auto_use_fixtures,
                params,
                fixture_params: self.fixture_params.as_ref().map(Rc::clone),
                parameter_scopes: self.parameter_scopes.as_ref().map(Rc::clone),
                parameter_id: self
                    .identity
                    .as_ref()
                    .map(|identity| identity.node.as_str()),
            },
        );
        PreparedTestAttempt {
            fixtures,
            setup_duration: setup_start.elapsed(),
            output_capture,
        }
    }

    /// Prepares a retry under its setup coverage context.
    fn prepare_retry(
        &self,
        params: HashMap<String, Arc<Py<PyAny>>>,
        settings: &VariantSettings,
    ) -> PreparedTestAttempt {
        self.set_coverage_context(&settings.qualified_name, CoveragePhase::Setup);
        self.prepare_attempt(params, self.start_output_capture())
    }

    /// Derives identity and execution policy after first fixture setup.
    fn settings(
        &self,
        function_arguments: &FixtureArguments,
        request_tags: &RuntimeTags,
    ) -> VariantSettings {
        let name = self.test.name();
        let fixture_names = self
            .fixture_dependencies
            .iter()
            .map(|fixture_id| self.fixture_plan.fixture(*fixture_id).function_name())
            .collect::<Vec<_>>();
        let mut framework_fixture_names = self
            .fixture_dependencies
            .iter()
            .map(|fixture_id| self.fixture_plan.fixture(*fixture_id))
            .filter(|fixture| fixture.name().module_path().module_name() == "karva._builtins")
            .map(NormalizedFixture::function_name)
            .collect::<Vec<_>>();
        framework_fixture_names.push("request");
        let parameters = if let Some(id) = self
            .identity
            .as_ref()
            .and_then(|identity| identity.display.as_ref())
        {
            Some(id.clone())
        } else {
            test_parameters(
                self.py,
                function_arguments,
                &self.test.statement().parameters,
                &framework_fixture_names,
            )
        };
        let qualified_test_name = if let Some(parameters) = parameters {
            QualifiedTestName::with_parameters(name.clone(), parameters)
        } else {
            QualifiedTestName::new(name.clone())
        };
        let qualified_name = qualified_test_name.to_string();
        let mut tags = self.tags.clone();
        tags.extend_runtime(request_tags);
        let custom_tag_names = tags.custom_tag_names();
        let evaluation_context = EvalContext {
            test_name: &qualified_name,
            tags: &custom_tag_names,
        };
        let fail_slow_budget = tags
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
        let timeout_seconds = tags.timeout_tag().map(TimeoutTag::seconds).or_else(|| {
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
        let flaky_result = self
            .package_runner
            .context
            .settings()
            .flaky_result_for(&evaluation_context);
        let junit_flaky_fail_status = self
            .package_runner
            .context
            .settings()
            .junit_flaky_fail_status_for(&evaluation_context);
        let expect_fail_tag = tags.expect_fail_tag();
        let async_patch_result = if self.test.statement().is_async {
            crate::utils::patch_async_test_function(self.py, &self.test.py_function)
        } else {
            Ok(false)
        };
        let is_async = self.test.statement().is_async && matches!(&async_patch_result, Ok(false));
        let mut snapshot_test_name = name.function_name().to_string();
        if let Some(parameters) = test_parameters(
            self.py,
            function_arguments,
            &self.test.statement().parameters,
            &fixture_names,
        ) {
            snapshot_test_name.push('(');
            snapshot_test_name.push_str(&parameters);
            snapshot_test_name.push(')');
        }
        let snapshot_context =
            SnapshotContext::new(self.module_path.to_string(), snapshot_test_name);

        VariantSettings {
            qualified_test_name,
            qualified_name,
            snapshot_context,
            expect_fail_tag,
            async_patch_result,
            is_async,
            timeout_seconds,
            fail_slow_budget,
            slow_timeout,
            max_attempts,
            flaky_result,
            junit_flaky_fail_status,
        }
    }

    /// Registers final outcome after all retries and returns pass/fail status.
    fn finish(
        &mut self,
        settings: &VariantSettings,
        prior_attempts: Vec<TestLifecycleAttempt>,
        final_attempt: TestLifecycleAttempt,
    ) -> bool {
        self.clear_coverage_context();
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
            self.package_runner.state.report_test_attempt(
                self.package_runner.context,
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
            self.package_runner.state.register_slow_test(
                self.package_runner.context,
                &settings.qualified_test_name,
                total_duration,
            );
        }
        self.package_runner
            .context
            .report_test_finished(&settings.qualified_test_name);

        if prior_attempts.is_empty() {
            self.package_runner.state.register_test_case_result(
                self.package_runner.context,
                &settings.qualified_test_name,
                final_attempt.outcome,
                total_duration,
                captured_output,
            )
        } else {
            let final_attempt_number = final_attempt.attempt;
            let outcome = final_attempt.outcome.clone();
            let flaky_failure = matches!(&outcome, TestExecutionOutcome::Passed)
                && settings.flaky_result == FlakyResult::Fail;
            let junit_flaky_failure =
                flaky_failure && settings.junit_flaky_fail_status == JunitFlakyFailStatus::Failure;
            let mut execution_attempts = prior_attempts
                .into_iter()
                .map(TestLifecycleAttempt::into_execution_attempt)
                .collect::<Vec<_>>();
            execution_attempts.push(final_attempt.into_execution_attempt());
            self.package_runner.state.register_retried_result(
                &settings.qualified_test_name,
                outcome,
                total_duration,
                TestCaseRetry::new(final_attempt_number, settings.max_attempts)
                    .with_failure_policy(flaky_failure, junit_flaky_failure),
                captured_output,
                execution_attempts,
            )
        }
    }

    /// Returns a registered skip result when filters or skip policy exclude this variant.
    fn should_skip(&mut self) -> Option<bool> {
        let filter = &self.package_runner.context.settings().test().filter;
        let run_ignored = self.package_runner.context.settings().test().run_ignored;

        if !filter.is_empty() {
            let qualified = QualifiedTestName::new(self.test.name().clone());
            let display_name = qualified.to_string();
            let custom_names = self.tags.custom_tag_names();
            let context = EvalContext {
                test_name: &display_name,
                tags: &custom_names,
            };
            if !filter.matches(&context) {
                return Some(self.package_runner.state.register_test_case_result(
                    self.package_runner.context,
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
        let qualified = QualifiedTestName::new(self.test.name().clone());
        Some(self.package_runner.state.register_test_case_result(
            self.package_runner.context,
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

    /// Marks setup whose full fixture-derived test identity is not known yet.
    fn begin_pending_coverage_setup(&self) {
        if let Some(coverage) = self.package_runner.coverage {
            coverage.begin_pending_test_setup(self.py);
        }
    }

    /// Resolves pending setup observations to the final qualified test identity.
    fn resolve_pending_coverage_setup(&self, test: &str) {
        if let Some(coverage) = self.package_runner.coverage {
            coverage.resolve_pending_test_setup(self.py, test);
        }
    }

    /// Updates coverage attribution for one test lifecycle phase.
    fn set_coverage_context(&self, test: &str, phase: CoveragePhase) {
        if let Some(coverage) = self.package_runner.coverage {
            coverage.set_test_context(self.py, test, phase);
        }
    }

    /// Restores session attribution between test variants.
    fn clear_coverage_context(&self) {
        if let Some(coverage) = self.package_runner.coverage {
            coverage.clear_test_context(self.py);
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
    /// Whether a pass after retry fails the run.
    flaky_result: FlakyResult,
    /// How a flaky failure appears in `JUnit`.
    junit_flaky_fail_status: JunitFlakyFailStatus,
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
