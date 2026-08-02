use std::time::Duration;

use karva_python_semantic::QualifiedTestName;
use ruff_db::diagnostic::Diagnostic;
use serde::{Deserialize, Serialize};

use super::diagnostic::RenderedDiagnostic;
use super::kind::IndividualTestResultKind;
use super::output::CapturedTestOutput;

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde skip_serializing_if passes a reference to the field"
)]
fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Final outcome and complete attempt history for one collected test variant.
pub struct TestCaseResult<D = RenderedDiagnostic> {
    module_name: String,
    name: String,
    full_name: String,
    outcome: TestCaseOutcome<D>,
    duration: Duration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry: Option<TestCaseRetry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    captured_output: Option<CapturedTestOutput>,
    #[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
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
        let function_name = test_case_name.function_name();
        let module_name = function_name.module_path().module_name().to_string();
        let full_name = test_case_name.to_string();
        let name = test_case_name.parameters().map_or_else(
            || function_name.function_name().to_string(),
            |parameters| format!("{}({parameters})", function_name.function_name()),
        );

        Self {
            module_name,
            name,
            full_name,
            outcome,
            duration,
            retry: None,
            captured_output,
            attempts: Vec::new(),
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
        result.retry = Some(retry);
        result.attempts = attempts;
        result
    }

    /// Builds a synthetic result when no semantic [`QualifiedTestName`] is available.
    pub fn from_display_name(
        full_name: &str,
        outcome: TestCaseOutcome<D>,
        duration: Duration,
        captured_output: Option<CapturedTestOutput>,
    ) -> Self {
        let (module_name, name) = full_name
            .split_once("::")
            .map_or(("unknown", full_name), |(module_name, name)| {
                (module_name, name)
            });

        Self {
            module_name: module_name.to_string(),
            name: name.to_string(),
            full_name: full_name.to_string(),
            outcome,
            duration,
            retry: None,
            captured_output,
            attempts: Vec::new(),
        }
    }

    pub fn module_name(&self) -> &str {
        &self.module_name
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn full_name(&self) -> &str {
        &self.full_name
    }

    pub fn outcome(&self) -> &TestCaseOutcome<D> {
        &self.outcome
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn retry(&self) -> Option<&TestCaseRetry> {
        self.retry.as_ref()
    }

    pub fn is_flaky_failure(&self) -> bool {
        self.retry
            .as_ref()
            .is_some_and(TestCaseRetry::is_flaky_failure)
    }

    pub fn is_junit_flaky_failure(&self) -> bool {
        self.retry
            .as_ref()
            .is_some_and(TestCaseRetry::is_junit_flaky_failure)
    }

    pub fn captured_output(&self) -> Option<&CapturedTestOutput> {
        self.captured_output.as_ref()
    }

    pub fn attempts(&self) -> &[TestCaseAttempt<D>] {
        &self.attempts
    }

    /// Converts every diagnostic while preserving outcome and retry structure.
    pub fn try_map_diagnostic<T, E>(
        self,
        mut map: impl FnMut(&D) -> Result<T, E>,
    ) -> Result<TestCaseResult<T>, E> {
        Ok(TestCaseResult {
            module_name: self.module_name,
            name: self.name,
            full_name: self.full_name,
            outcome: self.outcome.try_map_diagnostic(&mut map)?,
            duration: self.duration,
            retry: self.retry,
            captured_output: self.captured_output,
            attempts: self
                .attempts
                .into_iter()
                .map(|attempt| attempt.try_map_diagnostic(&mut map))
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Outcome, duration, and output captured for one retry attempt.
pub struct TestCaseAttempt<D = RenderedDiagnostic> {
    attempt: u32,
    outcome: TestCaseOutcome<D>,
    duration: Duration,
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

    pub fn attempt(&self) -> u32 {
        self.attempt
    }

    pub fn outcome(&self) -> &TestCaseOutcome<D> {
        &self.outcome
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn captured_output(&self) -> Option<&CapturedTestOutput> {
        self.captured_output.as_ref()
    }

    fn try_map_diagnostic<T, E>(
        self,
        mut map: impl FnMut(&D) -> Result<T, E>,
    ) -> Result<TestCaseAttempt<T>, E> {
        Ok(TestCaseAttempt {
            attempt: self.attempt,
            outcome: self.outcome.try_map_diagnostic(&mut map)?,
            duration: self.duration,
            captured_output: self.captured_output,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Retry summary and policies applied to a final test result.
pub struct TestCaseRetry {
    attempts: u32,
    max_attempts: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    flaky_failure: bool,
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

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }

    pub fn is_flaky_failure(&self) -> bool {
        self.flaky_failure
    }

    pub fn is_junit_flaky_failure(&self) -> bool {
        self.junit_flaky_failure
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Semantic outcome of one test, parameterized by diagnostic representation.
pub enum TestCaseOutcome<D = RenderedDiagnostic> {
    /// Test completed without failure.
    Passed,

    /// Assertion or explicit test failure.
    Failed {
        /// Primary failure diagnostic.
        diagnostic: D,

        /// Additional diagnostics belonging to the same failure.
        #[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
        related: Vec<D>,
    },

    /// Collection, fixture, or execution error rather than a test assertion failure.
    Error {
        /// Primary error diagnostic.
        diagnostic: D,

        /// Additional diagnostics belonging to the same error.
        #[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
        related: Vec<D>,

        /// Fixture failures that explain how this error reached the test.
        #[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
        fixture_failures: Vec<FixtureFailure>,
    },

    /// Test intentionally did not execute.
    Skipped {
        /// User-provided skip reason, when available.
        reason: Option<String>,
    },
}

impl<D> TestCaseOutcome<D> {
    /// Creates a failure with no related diagnostics.
    pub fn failed(diagnostic: D) -> Self {
        Self::Failed {
            diagnostic,
            related: Vec::new(),
        }
    }

    /// Creates an execution error with no related diagnostics.
    pub fn error(diagnostic: D) -> Self {
        Self::error_with_related(diagnostic, Vec::new())
    }

    /// Creates an execution error retaining secondary diagnostics.
    pub fn error_with_related(diagnostic: D, related: Vec<D>) -> Self {
        Self::error_with_fixture_failures(diagnostic, related, Vec::new())
    }

    /// Creates an execution error with its fixture dependency context.
    pub fn error_with_fixture_failures(
        diagnostic: D,
        related: Vec<D>,
        fixture_failures: Vec<FixtureFailure>,
    ) -> Self {
        Self::Error {
            diagnostic,
            related,
            fixture_failures,
        }
    }

    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    pub fn is_non_success(&self) -> bool {
        matches!(self, Self::Failed { .. } | Self::Error { .. })
    }

    pub fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped { .. })
    }

    pub fn diagnostic(&self) -> Option<&D> {
        match self {
            Self::Failed { diagnostic, .. } | Self::Error { diagnostic, .. } => Some(diagnostic),
            Self::Passed | Self::Skipped { .. } => None,
        }
    }

    pub fn related_diagnostics(&self) -> &[D] {
        match self {
            Self::Failed { related, .. } | Self::Error { related, .. } => related,
            Self::Passed | Self::Skipped { .. } => &[],
        }
    }

    /// Returns fixture failures attached to execution errors.
    pub fn fixture_failures(&self) -> &[FixtureFailure] {
        match self {
            Self::Error {
                fixture_failures, ..
            } => fixture_failures,
            Self::Passed | Self::Failed { .. } | Self::Skipped { .. } => &[],
        }
    }

    /// Maps this semantic outcome into its reporting and statistics category.
    pub fn result_kind(&self) -> IndividualTestResultKind {
        match self {
            Self::Passed => IndividualTestResultKind::Passed,
            Self::Failed { .. } => IndividualTestResultKind::Failed,
            Self::Error { .. } => IndividualTestResultKind::Error,
            Self::Skipped { reason } => IndividualTestResultKind::Skipped {
                reason: reason.clone(),
            },
        }
    }

    fn try_map_diagnostic<T, E>(
        self,
        mut map: impl FnMut(&D) -> Result<T, E>,
    ) -> Result<TestCaseOutcome<T>, E> {
        Ok(match self {
            Self::Passed => TestCaseOutcome::Passed,
            Self::Failed {
                diagnostic,
                related,
            } => TestCaseOutcome::Failed {
                diagnostic: map(&diagnostic)?,
                related: related
                    .into_iter()
                    .map(|diagnostic| map(&diagnostic))
                    .collect::<Result<Vec<_>, _>>()?,
            },
            Self::Error {
                diagnostic,
                related,
                fixture_failures,
            } => TestCaseOutcome::Error {
                diagnostic: map(&diagnostic)?,
                related: related
                    .into_iter()
                    .map(|diagnostic| map(&diagnostic))
                    .collect::<Result<Vec<_>, _>>()?,
                fixture_failures,
            },
            Self::Skipped { reason } => TestCaseOutcome::Skipped { reason },
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Fixture setup failure and the dependency path that exposed it to a test.
pub struct FixtureFailure {
    /// Fixture whose setup failed.
    fixture: String,

    /// How the test acquired the fixture.
    usage: FixtureUsage,

    /// Fixture dependency path from the test to the failure.
    dependency_chain: Vec<String>,
}

impl FixtureFailure {
    /// Records a fixture failure and how it reached the test.
    pub fn new(fixture: String, usage: FixtureUsage, dependency_chain: Vec<String>) -> Self {
        Self {
            fixture,
            usage,
            dependency_chain,
        }
    }

    pub fn fixture(&self) -> &str {
        &self.fixture
    }

    pub fn usage(&self) -> FixtureUsage {
        self.usage
    }

    pub fn dependency_chain(&self) -> &[String] {
        &self.dependency_chain
    }

    /// Describes the failed fixture relationship for user-facing diagnostics.
    pub fn description(&self) -> String {
        match self.usage {
            FixtureUsage::Required => format!("requires fixture `{}`", self.fixture),
            FixtureUsage::UseFixtures => {
                format!("uses fixture `{}` via `use_fixtures`", self.fixture)
            }
            FixtureUsage::AutoUse => format!("uses auto-use fixture `{}`", self.fixture),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Mechanism through which a test depends on a fixture.
pub enum FixtureUsage {
    /// Fixture appears as a test or fixture parameter.
    Required,

    /// Fixture was requested by `@karva.tags.use_fixtures`.
    UseFixtures,

    /// Fixture applies automatically without an explicit request.
    AutoUse,
}

/// Worker-side test result retaining Ruff diagnostics.
pub type TestExecutionResult = TestCaseResult<Diagnostic>;
/// Worker-side test outcome retaining Ruff diagnostics.
pub type TestExecutionOutcome = TestCaseOutcome<Diagnostic>;
/// Worker-side retry attempt retaining Ruff diagnostics.
pub type TestExecutionAttempt = TestCaseAttempt<Diagnostic>;

#[cfg(test)]
mod tests {
    use karva_python_semantic::{ModulePath, QualifiedFunctionName};

    use super::*;

    #[test]
    fn test_case_result_uses_structured_parameterized_name() {
        let name = QualifiedTestName::with_parameters(
            QualifiedFunctionName::new(
                "test_example".to_string(),
                ModulePath::new_with_name("test.py", "tests.test".to_string()),
            ),
            "value=1".to_string(),
        );

        let result =
            TestCaseResult::<()>::new(&name, TestCaseOutcome::Passed, Duration::ZERO, None);

        assert_eq!(result.module_name(), "tests.test");
        assert_eq!(result.name(), "test_example(value=1)");
        assert_eq!(result.full_name(), "tests.test::test_example(value=1)");
    }
}
