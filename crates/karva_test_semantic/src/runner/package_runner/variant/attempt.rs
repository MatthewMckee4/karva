//! Setup, Python call, teardown, and retry decision for one test attempt.

use std::time::{Duration, Instant};

use karva_coverage::CoveragePhase;
use karva_diagnostic::{CapturedTestOutput, TestExecutionAttempt, TestExecutionOutcome};
use pyo3::prelude::*;

use crate::extensions::functions::snapshot::set_snapshot_context;
use crate::utils::{run_coroutine, run_test_with_timeout};

use super::{VariantRunner, VariantSettings, finish_output_capture};
use crate::output_capture::PythonOutputCapture;
use crate::runner::package_runner::fixture::PreparedFixtures;
use crate::runner::package_runner::outcome::{
    OutcomeContext, PhaseDurations, apply_fail_slow_budget, attach_related_diagnostics,
    classify_test_result, reject_non_none_return,
};

impl VariantRunner<'_, '_, '_, '_, '_> {
    /// Runs one setup/call/teardown lifecycle and decides retry eligibility.
    pub(super) fn execute_attempt(
        &self,
        settings: &VariantSettings,
        function: &Py<PyAny>,
        test_name_env_result: &PyResult<()>,
        attempt_env_result: PyResult<()>,
        prepared: PreparedTestAttempt,
        attempt_number: u32,
    ) -> AttemptResult {
        let PreparedTestAttempt {
            fixtures:
                PreparedFixtures {
                    function_arguments,
                    setup_result,
                    request_tags: _,
                },
            setup_duration,
            output_capture,
        } = prepared;

        let body = match setup_result {
            Ok(()) => self.execute_test_body(
                settings,
                function,
                test_name_env_result,
                attempt_env_result,
                &function_arguments,
            ),
            Err(error) => AttemptBody {
                outcome: error
                    .into_test_error(self.py, self.package_runner.context.is_verbose())
                    .into_outcome(),
                call_duration: Duration::ZERO,
                retryable: true,
            },
        };

        let skipped = body.outcome.is_skipped();
        self.set_coverage_context(&settings.qualified_name, CoveragePhase::Teardown);
        let teardown_start = Instant::now();
        let finalizer_diagnostics = self.package_runner.clean_up_test_attempt(self.py);
        let teardown_failed = !finalizer_diagnostics.is_empty();
        let phases = PhaseDurations {
            setup: setup_duration,
            call: body.call_duration,
            teardown: teardown_start.elapsed(),
        };
        let duration = phases.total();
        let budget_exceeded = settings
            .fail_slow_budget
            .is_some_and(|budget| duration > budget);
        let outcome = attach_related_diagnostics(body.outcome, finalizer_diagnostics);
        let outcome = apply_fail_slow_budget(
            outcome,
            duration,
            phases,
            settings.fail_slow_budget,
            self.test.source_file(),
            self.test.statement(),
        );
        let retryable = body.retryable || teardown_failed || (budget_exceeded && !skipped);

        let captured_output = finish_output_capture(self.py, output_capture);

        AttemptResult {
            lifecycle: TestLifecycleAttempt {
                attempt: attempt_number,
                outcome,
                duration,
                captured_output,
            },
            retryable,
        }
    }

    /// Executes and classifies the Python test body after successful setup.
    fn execute_test_body(
        &self,
        settings: &VariantSettings,
        function: &Py<PyAny>,
        test_name_env_result: &PyResult<()>,
        attempt_env_result: PyResult<()>,
        function_arguments: &crate::runner::FixtureArguments,
    ) -> AttemptBody {
        set_snapshot_context(settings.snapshot_context.clone());
        let prepared_call = attempt_env_result.and_then(|()| {
            if let Err(error) = test_name_env_result {
                return Err(error.clone_ref(self.py));
            }
            if let Err(error) = &settings.async_patch_result {
                return Err(error.clone_ref(self.py));
            }
            if function_arguments.is_empty() || settings.timeout_seconds.is_some() {
                Ok(None)
            } else {
                function_arguments.to_kwargs(self.py).map(Some)
            }
        });
        let (test_result, call_duration) = match prepared_call {
            Ok(keyword_arguments) => {
                let call_start = Instant::now();
                let result = if let Some(seconds) = settings.timeout_seconds {
                    run_test_with_timeout(
                        self.py,
                        function,
                        function_arguments,
                        settings.is_async,
                        seconds,
                        &settings.snapshot_context,
                    )
                } else {
                    let result = if let Some(keyword_arguments) = keyword_arguments {
                        function.call(self.py, (), Some(&keyword_arguments))
                    } else {
                        function.call0(self.py)
                    };
                    if settings.is_async {
                        result.and_then(|coroutine| run_coroutine(self.py, coroutine))
                    } else {
                        result
                    }
                };
                (
                    result.map(|value| reject_non_none_return(self.py, &value)),
                    call_start.elapsed(),
                )
            }
            Err(error) => (Err(error), Duration::ZERO),
        };
        let result = classify_test_result(
            self.py,
            test_result,
            &OutcomeContext {
                name: self.test.name(),
                source_file: self.test.source_file(),
                stmt_function_def: self.test.statement(),
                function_arguments,
                expect_fail_tag: settings.expect_fail_tag.as_ref(),
                verbose: self.package_runner.context.is_verbose(),
            },
        );

        AttemptBody {
            outcome: result.outcome,
            call_duration,
            retryable: result.retryable,
        }
    }
}

/// Test-body result before common teardown and duration policy are applied.
struct AttemptBody {
    /// Classified result before teardown diagnostics and budgets.
    outcome: TestExecutionOutcome,

    /// Time spent invoking the Python test function.
    call_duration: Duration,

    /// Whether test-body policy permits another attempt.
    retryable: bool,
}

/// Fixture setup and timing captured before one test attempt.
pub(super) struct PreparedTestAttempt {
    /// Arguments, setup failures, and function-scoped finalizers.
    pub(super) fixtures: PreparedFixtures,
    /// Duration of fixture and parameter preparation.
    pub(super) setup_duration: Duration,
    /// Python stdout and stderr capture spanning setup, call, and teardown.
    pub(super) output_capture: Option<PythonOutputCapture>,
}

/// Completed call lifecycle plus retry decision.
pub(super) struct AttemptResult {
    /// Outcome and duration retained for reporting.
    pub(super) lifecycle: TestLifecycleAttempt,
    /// Whether policy permits retrying this result.
    pub(super) retryable: bool,
}

/// Reportable result for one initial or retry attempt.
pub(super) struct TestLifecycleAttempt {
    /// One-based attempt number.
    pub(super) attempt: u32,
    /// Classified outcome after teardown and budget checks.
    pub(super) outcome: TestExecutionOutcome,
    /// Full setup, call, and teardown duration.
    pub(super) duration: Duration,
    /// Output captured only during this attempt.
    pub(super) captured_output: Option<CapturedTestOutput>,
}

impl TestLifecycleAttempt {
    /// Converts internal lifecycle state to diagnostic reporting state.
    pub(super) fn into_execution_attempt(self) -> TestExecutionAttempt {
        TestExecutionAttempt::new(
            self.attempt,
            self.outcome,
            self.duration,
            self.captured_output,
        )
    }
}
