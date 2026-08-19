//! Final result reporting and captured-output aggregation for one variant.

use std::time::Duration;

use karva_diagnostic::{
    CapturedTestOutput, TestCaseRetry, TestExecutionOutcome, TestExecutionResult,
};
use karva_metadata::{FlakyResult, JunitFlakyFailStatus};
use pyo3::prelude::*;

use crate::output_capture::PythonOutputCapture;

use super::{TestLifecycleAttempt, VariantRunner, VariantSettings};

impl VariantRunner<'_, '_, '_, '_, '_> {
    /// Registers final outcome after all retries and returns pass/fail status.
    pub(super) fn finish(
        &self,
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
            self.package_runner.context.report_test_attempt(
                &settings.identity.qualified_test_name,
                final_attempt.attempt,
                final_attempt.outcome.result_kind(),
                final_attempt.duration,
            );
        }
        if settings
            .retry
            .slow_timeout
            .is_some_and(|threshold| total_duration > threshold)
        {
            self.package_runner
                .context
                .register_slow_test(&settings.identity.qualified_test_name, total_duration);
        }
        if prior_attempts.is_empty() {
            self.package_runner.context.register_test_case_result(
                &settings.identity.qualified_test_name,
                final_attempt.outcome,
                total_duration,
                captured_output,
            )
        } else {
            let final_attempt_number = final_attempt.attempt;
            let outcome = final_attempt.outcome.clone();
            let flaky_failure = matches!(&outcome, TestExecutionOutcome::Passed)
                && settings.retry.flaky_result == FlakyResult::Fail;
            let junit_flaky_failure = flaky_failure
                && settings.retry.junit_flaky_fail_status == JunitFlakyFailStatus::Failure;
            let mut execution_attempts = prior_attempts
                .into_iter()
                .map(TestLifecycleAttempt::into_execution_attempt)
                .collect::<Vec<_>>();
            execution_attempts.push(final_attempt.into_execution_attempt());
            let test_case = TestExecutionResult::retried(
                &settings.identity.qualified_test_name,
                outcome,
                total_duration,
                TestCaseRetry::new(final_attempt_number, settings.retry.max_attempts)
                    .with_failure_policy(flaky_failure, junit_flaky_failure),
                captured_output,
                execution_attempts,
            );
            self.package_runner
                .context
                .register_retried_result(&settings.identity.qualified_test_name, test_case)
        }
    }
}

/// Finishes best-effort Python output capture.
pub(super) fn finish_output_capture(
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
