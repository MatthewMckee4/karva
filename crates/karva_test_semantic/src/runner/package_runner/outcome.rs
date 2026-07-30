//! Test-call classification, teardown diagnostics, and duration policy.

use std::time::Duration;

use karva_diagnostic::TestExecutionOutcome;
use karva_python_semantic::QualifiedFunctionName;
use pyo3::prelude::*;
use ruff_db::diagnostic::Diagnostic;
use ruff_python_ast::StmtFunctionDef;
use ruff_source_file::SourceFile;

use crate::diagnostic::{
    fail_slow_exceeded_diagnostic, missing_fixtures_diagnostic, test_failure_diagnostic,
    test_pass_on_expect_failure_diagnostic, test_returned_value_diagnostic,
};
use crate::extensions::fixtures::missing_arguments_from_error;
use crate::extensions::tags::expect_fail::ExpectFailTag;
use crate::extensions::tags::skip::{extract_skip_reason, is_skip_exception};
use crate::runner::FixtureArguments;
use crate::utils::truncate_string;

/// Python-level result returned by a successful test function call.
pub(super) enum TestCallOutcome {
    /// Test followed Python convention and returned `None`.
    ReturnedNone,
    /// Test returned a value that must be reported as a failure.
    ReturnedValue(String),
}

/// Immutable inputs needed to turn one Python call result into a Karva outcome.
pub(super) struct OutcomeContext<'a> {
    /// Qualified function name used for missing-argument detection.
    pub(super) name: &'a QualifiedFunctionName,
    /// Source containing the test definition.
    pub(super) source_file: &'a SourceFile,
    /// Test definition used to locate diagnostics.
    pub(super) stmt_function_def: &'a StmtFunctionDef,
    /// Fixture and parameter values supplied to the test.
    pub(super) function_arguments: &'a FixtureArguments,
    /// Active expected-failure policy, when configured.
    pub(super) expect_fail_tag: Option<ExpectFailTag>,
}

/// Durations for each phase of one complete test attempt.
#[derive(Clone, Copy)]
pub(super) struct PhaseDurations {
    /// Time spent preparing fixtures and parameters.
    pub(super) setup: Duration,
    /// Time spent inside the Python test call.
    pub(super) call: Duration,
    /// Time spent running function-scoped fixture teardown.
    pub(super) teardown: Duration,
}

impl PhaseDurations {
    /// Returns total lifecycle duration without overflowing.
    pub(super) fn total(self) -> Duration {
        self.setup
            .saturating_add(self.call)
            .saturating_add(self.teardown)
    }

    /// Returns phase name whose duration was longest.
    fn slowest(self) -> &'static str {
        let mut slowest = ("setup", self.setup);
        for candidate in [("call", self.call), ("teardown", self.teardown)] {
            if candidate.1 > slowest.1 {
                slowest = candidate;
            }
        }
        slowest.0
    }
}

/// Converts Python's return convention into an explicit call outcome.
pub(super) fn reject_non_none_return(py: Python<'_>, value: &Py<PyAny>) -> TestCallOutcome {
    if value.bind(py).is_none() {
        TestCallOutcome::ReturnedNone
    } else {
        TestCallOutcome::ReturnedValue(returned_value_repr(py, value))
    }
}

/// Returns whether a call result qualifies for another attempt.
pub(super) fn should_retry_result(
    py: Python<'_>,
    test_result: &PyResult<TestCallOutcome>,
    expect_fail: bool,
    test_name: &str,
) -> bool {
    if expect_fail {
        return false;
    }

    match test_result {
        Ok(TestCallOutcome::ReturnedNone) => false,
        Ok(TestCallOutcome::ReturnedValue(_)) => true,
        Err(error) => {
            !is_skip_exception(py, error)
                && missing_arguments_from_error(test_name, &error.to_string()).is_empty()
        }
    }
}

/// Classifies a Python call result and attaches source diagnostics.
pub(super) fn classify_test_result(
    py: Python<'_>,
    test_result: PyResult<TestCallOutcome>,
    context: &OutcomeContext<'_>,
) -> TestExecutionOutcome {
    let expect_fail = context
        .expect_fail_tag
        .as_ref()
        .is_some_and(ExpectFailTag::should_expect_fail);

    let error = match test_result {
        Ok(TestCallOutcome::ReturnedValue(_)) if expect_fail => {
            return TestExecutionOutcome::Passed;
        }
        Ok(TestCallOutcome::ReturnedValue(value)) => {
            let diagnostic = test_returned_value_diagnostic(
                context.source_file.clone(),
                context.stmt_function_def,
                &value,
            );
            return TestExecutionOutcome::failed(diagnostic);
        }
        Ok(TestCallOutcome::ReturnedNone) if expect_fail => {
            let reason = context
                .expect_fail_tag
                .as_ref()
                .and_then(ExpectFailTag::reason);
            let diagnostic = test_pass_on_expect_failure_diagnostic(
                context.source_file.clone(),
                context.stmt_function_def,
                reason,
            );
            return TestExecutionOutcome::failed(diagnostic);
        }
        Ok(TestCallOutcome::ReturnedNone) => return TestExecutionOutcome::Passed,
        Err(error) => error,
    };

    if is_skip_exception(py, &error) {
        return TestExecutionOutcome::Skipped {
            reason: extract_skip_reason(py, &error),
        };
    }

    if expect_fail {
        return TestExecutionOutcome::Passed;
    }

    let missing_arguments =
        missing_arguments_from_error(context.name.function_name(), &error.to_string());
    if missing_arguments.is_empty() {
        let diagnostic = test_failure_diagnostic(
            py,
            context.source_file,
            context.stmt_function_def,
            context.function_arguments,
            &error,
        );
        TestExecutionOutcome::failed(diagnostic)
    } else {
        let diagnostic = missing_fixtures_diagnostic(
            context.source_file.clone(),
            context.stmt_function_def,
            &missing_arguments,
            karva_python_semantic::FunctionKind::Test,
        );
        TestExecutionOutcome::error(diagnostic)
    }
}

/// Attaches teardown failures while preserving an existing primary failure.
pub(super) fn attach_finalizer_diagnostics(
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
            TestExecutionOutcome::error_with_related(diagnostic, diagnostics)
        }
    }
}

/// Applies a full-lifecycle `fail-slow` budget to one attempt outcome.
pub(super) fn apply_fail_slow_budget(
    outcome: TestExecutionOutcome,
    lifecycle_duration: Duration,
    phases: PhaseDurations,
    budget: Option<Duration>,
    source_file: &SourceFile,
    stmt_function_def: &StmtFunctionDef,
) -> TestExecutionOutcome {
    let Some(budget) = budget else {
        return outcome;
    };
    if lifecycle_duration <= budget {
        return outcome;
    }

    let diagnostic = fail_slow_exceeded_diagnostic(
        source_file.clone(),
        stmt_function_def,
        budget,
        lifecycle_duration,
        phases.slowest(),
    );

    match outcome {
        TestExecutionOutcome::Passed => TestExecutionOutcome::failed(diagnostic),
        TestExecutionOutcome::Skipped { reason } => TestExecutionOutcome::Skipped { reason },
        other => attach_finalizer_diagnostics(other, vec![diagnostic]),
    }
}

/// Produces bounded `repr()` text for a non-`None` return value.
fn returned_value_repr(py: Python<'_>, value: &Py<PyAny>) -> String {
    match value.bind(py).repr() {
        Ok(repr) => truncate_string(&repr.to_string()),
        Err(error) => {
            let error = truncate_string(&error.value(py).to_string());
            format!("<repr failed: {error}>")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::PhaseDurations;

    #[test]
    fn phase_durations_select_first_slowest_phase_on_ties() {
        let phases = PhaseDurations {
            setup: Duration::from_millis(2),
            call: Duration::from_millis(2),
            teardown: Duration::from_millis(1),
        };

        assert_eq!(phases.slowest(), "setup");
        assert_eq!(phases.total(), Duration::from_millis(5));
    }
}
