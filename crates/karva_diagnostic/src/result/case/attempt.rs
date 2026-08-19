//! Per-attempt results and retry policy retained by a final test case.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::Diagnostic;

use super::super::diagnostic::RenderedDiagnostic;
use super::super::output::CapturedTestOutput;
use super::outcome::TestCaseOutcome;

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if passes a reference to the field"
)]
fn is_false(value: &bool) -> bool {
    !*value
}

/// Outcome, duration, and output captured for one retry attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCaseAttempt<D = RenderedDiagnostic> {
    /// One-based attempt number.
    attempt: u32,

    /// Semantic outcome of this attempt.
    outcome: TestCaseOutcome<D>,

    /// Time spent in this attempt.
    duration: Duration,

    /// Output captured during this attempt, when non-empty.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    captured_output: Option<CapturedTestOutput>,
}

impl<D> TestCaseAttempt<D> {
    /// Records one numbered attempt; attempt numbers are one-based.
    pub fn new(
        attempt: u32,
        outcome: TestCaseOutcome<D>,
        duration: Duration,
        captured_output: Option<CapturedTestOutput>,
    ) -> Self {
        Self {
            attempt,
            outcome,
            duration,
            captured_output,
        }
    }

    /// Returns the one-based attempt number.
    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    /// Returns the semantic outcome of this attempt.
    pub fn outcome(&self) -> &TestCaseOutcome<D> {
        &self.outcome
    }

    /// Returns the time spent in this attempt.
    pub fn duration(&self) -> Duration {
        self.duration
    }

    /// Returns output captured during this attempt, when present.
    pub fn captured_output(&self) -> Option<&CapturedTestOutput> {
        self.captured_output.as_ref()
    }

    pub(super) fn map_diagnostic<T>(self, mut map: impl FnMut(&D) -> T) -> TestCaseAttempt<T> {
        TestCaseAttempt {
            attempt: self.attempt,
            outcome: self.outcome.map_diagnostic(&mut map),
            duration: self.duration,
            captured_output: self.captured_output,
        }
    }
}

/// Retry summary and policies applied to a final test result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCaseRetry {
    /// Attempts consumed by the completed test.
    attempts: u32,

    /// Maximum attempts allowed by retry policy.
    max_attempts: u32,

    /// Whether exhausting retries fails the Karva run.
    #[serde(default, skip_serializing_if = "is_false")]
    flaky_failure: bool,

    /// Whether exhausting retries is a failure in `JUnit` output.
    #[serde(default, skip_serializing_if = "is_false")]
    junit_flaky_failure: bool,
}

impl TestCaseRetry {
    /// Records attempts consumed and maximum attempts permitted.
    pub fn new(attempts: u32, max_attempts: u32) -> Self {
        Self {
            attempts,
            max_attempts,
            flaky_failure: false,
            junit_flaky_failure: false,
        }
    }

    /// Marks whether retrying fails Karva's run and `JUnit` outcomes.
    #[must_use]
    pub fn with_failure_policy(mut self, flaky_failure: bool, junit_flaky_failure: bool) -> Self {
        self.flaky_failure = flaky_failure;
        self.junit_flaky_failure = junit_flaky_failure;
        self
    }

    /// Returns the number of attempts consumed by the completed test.
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Returns the maximum attempts allowed by retry policy.
    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    pub(super) fn is_flaky_failure(&self) -> bool {
        self.flaky_failure
    }

    pub(super) fn is_junit_flaky_failure(&self) -> bool {
        self.junit_flaky_failure
    }
}

/// Worker-side retry attempt retaining structured diagnostics.
pub type TestExecutionAttempt = TestCaseAttempt<Diagnostic>;
