//! Synchronous call deadlines enforced independently of Python and its GIL.
//!
//! Expiry flushes an attributed timeout result and exits the worker immediately.
//! Recovery and retries belong to the controller; arbitrary Python and native
//! calls cannot be interrupted safely in the current interpreter.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use karva_diagnostic::{
    TestCaseRetry, TestExecutionAttempt, TestExecutionOutcome, TestExecutionResult,
};
use karva_metadata::{FlakyResult, JunitFlakyFailStatus};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::diagnostic::test_timeout_diagnostic;
use crate::output_capture::SharedCapturedOutput;

use super::{TestLifecycleAttempt, VariantRunner, VariantSettings};

/// Lifecycle data retained if a synchronous call kills its interpreter.
pub(super) struct TimeoutAttempt<'a> {
    /// One-based attempt, including progress recovered by the controller.
    pub(super) attempt_number: u32,

    /// Setup time included in reports but excluded from the deadline.
    pub(super) setup_duration: Duration,

    /// Earlier attempts executed by this interpreter, before any replacement.
    pub(super) prior_attempts: &'a [TestLifecycleAttempt],

    /// Native stream mirrors readable while Python holds its GIL.
    pub(super) shared_output: Option<SharedCapturedOutput>,
}

impl VariantRunner<'_, '_, '_, '_, '_> {
    pub(super) fn run_sync_with_deadline(
        &self,
        settings: &VariantSettings,
        function: &Py<PyAny>,
        kwargs: Option<&Bound<'_, PyDict>>,
        seconds: f64,
        attempt: TimeoutAttempt<'_>,
    ) -> PyResult<Py<PyAny>> {
        let limit = Duration::try_from_secs_f64(seconds).map_err(|error| {
            PyValueError::new_err(format!("invalid test timeout {seconds}: {error}"))
        })?;
        let diagnostic = test_timeout_diagnostic(self.input.test.definition(), seconds);
        let reporter = self.package_runner.context.reporter();
        let name = settings.identity.qualified_test_name.clone();
        let previous = attempt
            .prior_attempts
            .iter()
            .cloned()
            .map(TestLifecycleAttempt::into_execution_attempt)
            .collect::<Vec<_>>();
        let max_attempts = settings.retry.max_attempts;
        let fail_on_flaky = settings.retry.flaky_result == FlakyResult::Fail;
        let junit_fail_on_flaky =
            settings.retry.junit_flaky_fail_status == JunitFlakyFailStatus::Failure;
        let (finished, receiver) = mpsc::channel();
        std::thread::scope(|scope| {
            let started = Instant::now();
            let watchdog = std::thread::Builder::new()
                .name("karva-test-deadline".to_owned())
                .spawn_scoped(scope, move || {
                    if matches!(
                        receiver.recv_timeout(limit),
                        Err(mpsc::RecvTimeoutError::Timeout)
                    ) {
                        let duration = attempt.setup_duration.saturating_add(started.elapsed());
                        let output = attempt
                            .shared_output
                            .as_ref()
                            .and_then(SharedCapturedOutput::snapshot);
                        let outcome = TestExecutionOutcome::failed(diagnostic);
                        let total = previous
                            .iter()
                            .map(TestExecutionAttempt::duration)
                            .sum::<Duration>()
                            .saturating_add(duration);
                        let mut attempts = previous;
                        attempts.push(TestExecutionAttempt::new(
                            attempt.attempt_number,
                            outcome.clone(),
                            duration,
                            output.clone(),
                        ));
                        let result = TestExecutionResult::retried(
                            &name,
                            outcome,
                            total,
                            TestCaseRetry::new(attempt.attempt_number, max_attempts),
                            output,
                            attempts,
                        );
                        reporter.report_test_timed_out(
                            &name,
                            result,
                            fail_on_flaky,
                            junit_fail_on_flaky,
                        );
                        // No Python shutdown: it could wait forever for the call being stopped.
                        std::process::exit(1);
                    }
                })
                .map_err(|error| {
                    PyRuntimeError::new_err(format!(
                        "failed to start test deadline watchdog: {error}"
                    ))
                })?;
            let result = function.call(self.py, (), kwargs);
            let _ = finished.send(());
            watchdog
                .join()
                .map_err(|_| PyRuntimeError::new_err("test deadline watchdog panicked"))?;
            result
        })
    }
}
