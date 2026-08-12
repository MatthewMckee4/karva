//! Test-call classification, teardown diagnostics, and duration policy.

use std::time::Duration;

use karva_diagnostic::{Diagnostic, TestExecutionOutcome};
use pyo3::prelude::*;

use crate::diagnostic::{
    fail_slow_exceeded_diagnostic, missing_fixtures_diagnostic, test_failure_diagnostic,
    test_pass_on_expect_failure_diagnostic, test_returned_value_diagnostic,
};
use crate::discovery::models::definition::TestDefinition;
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
    /// Test identity and source location used for diagnostics.
    pub(super) definition: &'a TestDefinition,
    /// Fixture and parameter values supplied to the test.
    pub(super) function_arguments: &'a FixtureArguments,
    /// Active expected-failure policy, when configured.
    pub(super) expect_fail_tag: Option<&'a ExpectFailTag>,
    /// Whether failure diagnostics should include every Python call frame.
    pub(super) verbose: bool,
}

/// Classified test-body outcome and whether retry policy may run it again.
pub(super) struct ClassifiedTestResult {
    /// User-visible semantic outcome.
    pub(super) outcome: TestExecutionOutcome,
    /// Whether another attempt could change this outcome.
    pub(super) retryable: bool,
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

/// Classifies a Python call result and its retry eligibility together.
pub(super) fn classify_test_result(
    py: Python<'_>,
    test_result: PyResult<TestCallOutcome>,
    context: &OutcomeContext<'_>,
) -> ClassifiedTestResult {
    let expect_fail = context
        .expect_fail_tag
        .is_some_and(ExpectFailTag::should_expect_fail);

    let error = match test_result {
        Ok(TestCallOutcome::ReturnedValue(_)) if expect_fail => {
            return ClassifiedTestResult::new(TestExecutionOutcome::Passed, false);
        }
        Ok(TestCallOutcome::ReturnedValue(value)) => {
            let diagnostic = test_returned_value_diagnostic(context.definition, &value);
            return ClassifiedTestResult::new(TestExecutionOutcome::failed(diagnostic), true);
        }
        Ok(TestCallOutcome::ReturnedNone) if expect_fail => {
            let reason = context.expect_fail_tag.and_then(ExpectFailTag::reason);
            let diagnostic = test_pass_on_expect_failure_diagnostic(context.definition, reason);
            return ClassifiedTestResult::new(TestExecutionOutcome::failed(diagnostic), false);
        }
        Ok(TestCallOutcome::ReturnedNone) => {
            return ClassifiedTestResult::new(TestExecutionOutcome::Passed, false);
        }
        Err(error) => error,
    };

    if is_skip_exception(py, &error) {
        return ClassifiedTestResult::new(
            TestExecutionOutcome::Skipped {
                reason: extract_skip_reason(py, &error),
            },
            false,
        );
    }

    if expect_fail {
        return ClassifiedTestResult::new(TestExecutionOutcome::Passed, false);
    }

    let missing_arguments = missing_arguments_from_error(
        context.definition.name().function_name(),
        &error.to_string(),
    );
    if missing_arguments.is_empty() {
        let diagnostic = test_failure_diagnostic(
            py,
            context.definition,
            context.function_arguments,
            &error,
            context.verbose,
        );
        ClassifiedTestResult::new(TestExecutionOutcome::failed(diagnostic), true)
    } else {
        let diagnostic = missing_fixtures_diagnostic(
            context.definition.source_file().clone(),
            context.definition.name().function_name(),
            context.definition.diagnostic_range(),
            &missing_arguments,
            karva_python_semantic::FunctionKind::Test,
        );
        ClassifiedTestResult::new(TestExecutionOutcome::error(diagnostic), false)
    }
}

impl ClassifiedTestResult {
    fn new(outcome: TestExecutionOutcome, retryable: bool) -> Self {
        Self { outcome, retryable }
    }
}

/// Attaches later diagnostics while preserving an existing primary failure.
pub(super) fn attach_related_diagnostics(
    outcome: TestExecutionOutcome,
    diagnostics: Vec<Diagnostic>,
) -> TestExecutionOutcome {
    let mut diagnostics = diagnostics.into_iter();
    let Some(first) = diagnostics.next() else {
        return outcome;
    };

    match outcome {
        TestExecutionOutcome::Failed {
            diagnostic,
            mut related,
        } => {
            related.push(first);
            related.extend(diagnostics);
            TestExecutionOutcome::Failed {
                diagnostic,
                related,
            }
        }
        TestExecutionOutcome::Error {
            diagnostic,
            mut related,
            fixture_failures,
        } => {
            related.push(first);
            related.extend(diagnostics);
            TestExecutionOutcome::Error {
                diagnostic,
                related,
                fixture_failures,
            }
        }
        TestExecutionOutcome::Passed | TestExecutionOutcome::Skipped { .. } => {
            TestExecutionOutcome::error_with_related(first, diagnostics.collect())
        }
    }
}

/// Applies a full-lifecycle `fail-slow` budget to one attempt outcome.
pub(super) fn apply_fail_slow_budget(
    outcome: TestExecutionOutcome,
    lifecycle_duration: Duration,
    phases: PhaseDurations,
    budget: Option<Duration>,
    definition: &TestDefinition,
) -> TestExecutionOutcome {
    let Some(budget) = budget else {
        return outcome;
    };
    if lifecycle_duration <= budget {
        return outcome;
    }

    let diagnostic =
        fail_slow_exceeded_diagnostic(definition, budget, lifecycle_duration, phases.slowest());

    match outcome {
        TestExecutionOutcome::Passed => TestExecutionOutcome::failed(diagnostic),
        TestExecutionOutcome::Skipped { reason } => TestExecutionOutcome::Skipped { reason },
        other => attach_related_diagnostics(other, vec![diagnostic]),
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
