//! Setup, Python call, teardown, and retry decision for one test attempt.

use std::time::{Duration, Instant};

use karva_diagnostic::{CapturedTestOutput, TestExecutionAttempt, TestExecutionOutcome};
use pyo3::prelude::*;

use crate::diagnostic::fixture_failure_diagnostic;
use crate::extensions::fixtures::FixtureScope;
use crate::extensions::functions::snapshot::set_snapshot_context;
use crate::utils::{run_coroutine, run_test_with_timeout};

use super::{VariantRunner, VariantSettings, finish_output_capture};
use crate::output_capture::PythonOutputCapture;
use crate::runner::package_runner::fixture::PreparedFixtures;
use crate::runner::package_runner::outcome::{
    OutcomeContext, PhaseDurations, apply_fail_slow_budget, attach_finalizer_diagnostics,
    classify_test_result, reject_non_none_return, should_retry_result,
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
                    fixture_call_errors,
                    test_finalizers,
                },
            setup_duration,
            output_capture,
        } = prepared;

        let (outcome, duration, retryable) = if fixture_call_errors.is_empty() {
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
                            &function_arguments,
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
            let retryable_result = should_retry_result(
                self.py,
                &test_result,
                settings.expect_fail,
                settings.qualified_test_name.function_name().function_name(),
            );
            let outcome = classify_test_result(
                self.py,
                test_result,
                &OutcomeContext {
                    name: &self.test.name,
                    source_file: &self.test.source_file,
                    stmt_function_def: &self.test.stmt_function_def,
                    function_arguments: &function_arguments,
                    expect_fail_tag: settings.expect_fail_tag.clone(),
                },
            );
            let skipped = outcome.is_skipped();

            let teardown_start = Instant::now();
            let mut finalizer_diagnostics = test_finalizers
                .into_iter()
                .rev()
                .filter_map(|finalizer| finalizer.run(self.py))
                .collect::<Vec<_>>();
            finalizer_diagnostics.extend(
                self.package_runner
                    .clean_up_scope(self.py, FixtureScope::Function),
            );
            let teardown_failed = !finalizer_diagnostics.is_empty();
            let phases = PhaseDurations {
                setup: setup_duration,
                call: call_duration,
                teardown: teardown_start.elapsed(),
            };
            let duration = phases.total();
            let budget_exceeded = settings
                .fail_slow_budget
                .is_some_and(|budget| duration > budget);
            let outcome = attach_finalizer_diagnostics(outcome, finalizer_diagnostics);
            let outcome = apply_fail_slow_budget(
                outcome,
                duration,
                phases,
                settings.fail_slow_budget,
                &self.test.source_file,
                &self.test.stmt_function_def,
            );
            (
                outcome,
                duration,
                retryable_result || teardown_failed || (budget_exceeded && !skipped),
            )
        } else {
            let mut diagnostics = fixture_call_errors
                .into_iter()
                .map(|error| fixture_failure_diagnostic(self.py, error))
                .collect::<Vec<_>>();
            let teardown_start = Instant::now();
            diagnostics.extend(
                test_finalizers
                    .into_iter()
                    .rev()
                    .filter_map(|finalizer| finalizer.run(self.py)),
            );
            diagnostics.extend(
                self.package_runner
                    .clean_up_scope(self.py, FixtureScope::Function),
            );
            let phases = PhaseDurations {
                setup: setup_duration,
                call: Duration::ZERO,
                teardown: teardown_start.elapsed(),
            };
            let duration = phases.total();
            let diagnostic = diagnostics.remove(0);
            let outcome = TestExecutionOutcome::error_with_related(diagnostic, diagnostics);
            let outcome = apply_fail_slow_budget(
                outcome,
                duration,
                phases,
                settings.fail_slow_budget,
                &self.test.source_file,
                &self.test.stmt_function_def,
            );
            (outcome, duration, true)
        };

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
