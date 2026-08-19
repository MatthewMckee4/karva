use std::time::Duration;

use camino::Utf8Path;
use karva_python_semantic::QualifiedTestName;

use crate::{Diagnostic, DisplayDiagnosticConfig, render_diagnostic};

use super::diagnostic::RenderedDiagnostic;
use super::output::CapturedTestOutput;

mod attempt;
mod outcome;
mod serialization;
#[cfg(test)]
mod tests;

pub use attempt::{TestCaseAttempt, TestCaseRetry, TestExecutionAttempt};
pub use outcome::{FixtureFailure, FixtureUsage, TestCaseOutcome, TestExecutionOutcome};

/// Final outcome and complete attempt history for one collected test variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestCaseResult<D = RenderedDiagnostic> {
    /// Names used to display and group the collected test.
    identity: TestCaseIdentity,

    /// Execution data produced after the test entered its final attempt.
    payload: TestCaseResultPayload<D>,
}

/// Display identity attached to a final test result.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TestCaseIdentity {
    /// Dotted Python module containing the test.
    module_name: String,

    /// Module-relative test name, including rendered parameters.
    name: String,

    /// Fully qualified user-visible test name.
    full_name: String,
}

/// Final execution data grouped separately from user-visible test identity.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TestCaseResultPayload<D = RenderedDiagnostic> {
    /// Final semantic outcome after retry policy has completed.
    outcome: TestCaseOutcome<D>,

    /// Total duration represented by this final result.
    duration: Duration,

    /// Retry policy and attempt count, when the test was retried.
    retry: Option<TestCaseRetry>,

    /// Output captured during the final attempt, when non-empty.
    captured_output: Option<CapturedTestOutput>,

    /// Earlier attempts retained when retry policy reran the test.
    attempts: Vec<TestCaseAttempt<D>>,
}

impl<D> TestCaseResult<D> {
    /// Builds a result for a test executed once.
    pub fn new(
        test_case_name: &QualifiedTestName,
        outcome: TestCaseOutcome<D>,
        duration: Duration,
        captured_output: Option<CapturedTestOutput>,
    ) -> Self {
        Self {
            identity: TestCaseIdentity::from_test_name(test_case_name),
            payload: TestCaseResultPayload::new(outcome, duration, captured_output),
        }
    }

    /// Builds a result whose final outcome followed earlier failed attempts.
    pub fn retried(
        test_case_name: &QualifiedTestName,
        outcome: TestCaseOutcome<D>,
        duration: Duration,
        retry: TestCaseRetry,
        captured_output: Option<CapturedTestOutput>,
        attempts: Vec<TestCaseAttempt<D>>,
    ) -> Self {
        let mut result = Self::new(test_case_name, outcome, duration, captured_output);
        result.payload.retry = Some(retry);
        result.payload.attempts = attempts;
        result
    }

    /// Builds a synthetic result when no semantic [`QualifiedTestName`] is available.
    pub(super) fn from_display_name(
        full_name: &str,
        outcome: TestCaseOutcome<D>,
        duration: Duration,
        captured_output: Option<CapturedTestOutput>,
    ) -> Self {
        Self {
            identity: TestCaseIdentity::from_display_name(full_name),
            payload: TestCaseResultPayload::new(outcome, duration, captured_output),
        }
    }

    /// Returns the dotted Python module containing the test.
    pub fn module_name(&self) -> &str {
        &self.identity.module_name
    }

    /// Returns the module-relative name, including rendered parameters.
    pub fn name(&self) -> &str {
        &self.identity.name
    }

    /// Returns the fully qualified user-visible test name.
    pub fn full_name(&self) -> &str {
        &self.identity.full_name
    }

    /// Returns the final semantic outcome.
    pub fn outcome(&self) -> &TestCaseOutcome<D> {
        &self.payload.outcome
    }

    /// Returns total duration represented by this result.
    pub fn duration(&self) -> Duration {
        self.payload.duration
    }

    /// Returns retry policy and attempt count when the test was retried.
    pub fn retry(&self) -> Option<&TestCaseRetry> {
        self.payload.retry.as_ref()
    }

    /// Whether retry exhaustion fails the Karva run.
    pub fn is_flaky_failure(&self) -> bool {
        self.payload
            .retry
            .as_ref()
            .is_some_and(TestCaseRetry::is_flaky_failure)
    }

    /// Whether retry exhaustion is a failure in `JUnit` output.
    pub fn is_junit_flaky_failure(&self) -> bool {
        self.payload
            .retry
            .as_ref()
            .is_some_and(TestCaseRetry::is_junit_flaky_failure)
    }

    /// Returns output captured during the final attempt, when present.
    pub fn captured_output(&self) -> Option<&CapturedTestOutput> {
        self.payload.captured_output.as_ref()
    }

    /// Returns earlier attempts retained by retry policy.
    pub fn attempts(&self) -> &[TestCaseAttempt<D>] {
        &self.payload.attempts
    }

    /// Converts every diagnostic while preserving outcome and retry structure.
    fn map_diagnostic<T>(self, mut map: impl FnMut(&D) -> T) -> TestCaseResult<T> {
        TestCaseResult {
            identity: self.identity,
            payload: self.payload.map_diagnostic(&mut map),
        }
    }
}

impl TestCaseIdentity {
    /// Builds identity directly from semantic worker-side names.
    fn from_test_name(test_case_name: &QualifiedTestName) -> Self {
        let function_name = test_case_name.function_name();
        let name = test_case_name.parameters().map_or_else(
            || function_name.function_name().to_string(),
            |parameters| format!("{}({parameters})", function_name.function_name()),
        );
        Self {
            module_name: function_name.module_path().module_name().to_string(),
            name,
            full_name: test_case_name.to_string(),
        }
    }

    /// Splits the canonical `module::test` display form.
    fn from_display_name(full_name: &str) -> Self {
        let (module_name, name) = full_name
            .split_once("::")
            .map_or(("unknown", full_name), |identity| identity);
        Self {
            module_name: module_name.to_string(),
            name: name.to_string(),
            full_name: full_name.to_string(),
        }
    }
}

impl<D> TestCaseResultPayload<D> {
    /// Builds execution data for a test executed once.
    fn new(
        outcome: TestCaseOutcome<D>,
        duration: Duration,
        captured_output: Option<CapturedTestOutput>,
    ) -> Self {
        Self {
            outcome,
            duration,
            retry: None,
            captured_output,
            attempts: Vec::new(),
        }
    }

    /// Converts every diagnostic while preserving retry structure.
    fn map_diagnostic<T>(self, mut map: impl FnMut(&D) -> T) -> TestCaseResultPayload<T> {
        TestCaseResultPayload {
            outcome: self.outcome.map_diagnostic(&mut map),
            duration: self.duration,
            retry: self.retry,
            captured_output: self.captured_output,
            attempts: self
                .attempts
                .into_iter()
                .map(|attempt| attempt.map_diagnostic(&mut map))
                .collect(),
        }
    }
}

impl TestCaseResult<Diagnostic> {
    /// Converts source-backed diagnostics into transport-safe renderings.
    pub fn render(
        self,
        cwd: &Utf8Path,
        config: DisplayDiagnosticConfig,
    ) -> TestCaseResult<RenderedDiagnostic> {
        self.map_diagnostic(|diagnostic| render_diagnostic(diagnostic, cwd, config))
    }
}

/// Worker-side test result retaining structured diagnostics.
pub type TestExecutionResult = TestCaseResult<Diagnostic>;
