//! Final result reporting and captured-output aggregation for one variant.

use std::time::Duration;

use karva_diagnostic::{
    CapturedTestOutput, TestCaseRetry, TestExecutionOutcome, TestExecutionResult,
};
use karva_metadata::{FlakyResult, JunitFlakyFailStatus};
use pyo3::prelude::*;

use crate::output_capture::{PythonOutputCapture, format_captured_bytes, retain_bounded_bytes};

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
        let attempt_output = prior_attempts
            .iter()
            .chain(std::iter::once(&final_attempt))
            .filter_map(|attempt| attempt.captured_output.as_ref());
        let captured_output = combine_captured_output(
            self.scope_output.iter().chain(attempt_output),
            self.package_runner
                .context
                .settings()
                .terminal()
                .output_limit,
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
            let output = CapturedTestOutput::with_raw_totals(
                output.stdout,
                output.stderr,
                output.stdout_raw,
                output.stderr_raw,
                output.stdout_total,
                output.stderr_total,
            );
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
    output_limit: usize,
) -> Option<CapturedTestOutput> {
    let mut outputs = outputs;
    let first = outputs.next()?;
    let Some(second) = outputs.next() else {
        return Some(first.clone());
    };

    let mut stdout_raw = first.raw_stdout().to_vec();
    let mut stderr_raw = first.raw_stderr().to_vec();
    let mut stdout_total = first.stdout_total();
    let mut stderr_total = first.stderr_total();
    for output in std::iter::once(second).chain(outputs) {
        stdout_total = stdout_total.saturating_add(output.stdout_total());
        stderr_total = stderr_total.saturating_add(output.stderr_total());
        let mut stdout_combined = stdout_raw;
        stdout_combined.extend_from_slice(output.raw_stdout());
        stdout_raw = retain_bounded_bytes(stdout_combined, output_limit);
        let mut stderr_combined = stderr_raw;
        stderr_combined.extend_from_slice(output.raw_stderr());
        stderr_raw = retain_bounded_bytes(stderr_combined, output_limit);
    }
    let stdout = format_captured_bytes(&stdout_raw, stdout_total, output_limit);
    let stderr = format_captured_bytes(&stderr_raw, stderr_total, output_limit);
    let output = CapturedTestOutput::with_raw_totals(
        stdout,
        stderr,
        stdout_raw,
        stderr_raw,
        stdout_total,
        stderr_total,
    );
    (!output.is_empty()).then_some(output)
}
