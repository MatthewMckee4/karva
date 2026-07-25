use std::time::Duration;

use karva_python_semantic::QualifiedTestName;
use ruff_db::diagnostic::Diagnostic;
use serde::{Deserialize, Serialize};

use super::diagnostic::RenderedDiagnostic;
use super::kind::IndividualTestResultKind;
use super::output::CapturedTestOutput;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub fn new(
        test_case_name: &QualifiedTestName,
        outcome: TestCaseOutcome<D>,
        duration: Duration,
        captured_output: Option<CapturedTestOutput>,
    ) -> Self {
        let function_name = test_case_name.function_name();
        let module_name = function_name.module_path().module_name().to_string();
        let full_name = test_case_name.to_string();
        let prefix = format!("{module_name}::");
        let name = full_name
            .strip_prefix(&prefix)
            .unwrap_or(&full_name)
            .to_string();

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

    pub fn captured_output(&self) -> Option<&CapturedTestOutput> {
        self.captured_output.as_ref()
    }

    pub fn attempts(&self) -> &[TestCaseAttempt<D>] {
        &self.attempts
    }

    pub fn try_map_diagnostic<T, E>(
        &self,
        mut map: impl FnMut(&D) -> Result<T, E>,
    ) -> Result<TestCaseResult<T>, E> {
        Ok(TestCaseResult {
            module_name: self.module_name.clone(),
            name: self.name.clone(),
            full_name: self.full_name.clone(),
            outcome: self.outcome.try_map_diagnostic(&mut map)?,
            duration: self.duration,
            retry: self.retry.clone(),
            captured_output: self.captured_output.clone(),
            attempts: self
                .attempts
                .iter()
                .map(|attempt| attempt.try_map_diagnostic(&mut map))
                .collect::<Result<Vec<_>, _>>()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCaseAttempt<D = RenderedDiagnostic> {
    attempt: u32,
    outcome: TestCaseOutcome<D>,
    duration: Duration,
}

impl<D> TestCaseAttempt<D> {
    pub fn new(attempt: u32, outcome: TestCaseOutcome<D>, duration: Duration) -> Self {
        Self {
            attempt,
            outcome,
            duration,
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

    fn try_map_diagnostic<T, E>(
        &self,
        mut map: impl FnMut(&D) -> Result<T, E>,
    ) -> Result<TestCaseAttempt<T>, E> {
        Ok(TestCaseAttempt {
            attempt: self.attempt,
            outcome: self.outcome.try_map_diagnostic(&mut map)?,
            duration: self.duration,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCaseRetry {
    attempts: u32,
    max_attempts: u32,
}

impl TestCaseRetry {
    pub fn new(attempts: u32, max_attempts: u32) -> Self {
        Self {
            attempts,
            max_attempts,
        }
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub fn max_attempts(&self) -> u32 {
        self.max_attempts
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestCaseOutcome<D = RenderedDiagnostic> {
    Passed,
    Failed {
        diagnostic: D,
        #[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
        related: Vec<D>,
    },
    Error {
        diagnostic: D,
        #[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
        related: Vec<D>,
    },
    Skipped {
        reason: Option<String>,
    },
}

impl<D> TestCaseOutcome<D> {
    pub fn failed(diagnostic: D) -> Self {
        Self::Failed {
            diagnostic,
            related: Vec::new(),
        }
    }

    pub fn error(diagnostic: D) -> Self {
        Self::Error {
            diagnostic,
            related: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_related(mut self, related: Vec<D>) -> Self {
        match &mut self {
            Self::Failed {
                related: existing, ..
            }
            | Self::Error {
                related: existing, ..
            } => *existing = related,
            Self::Passed | Self::Skipped { .. } => {}
        }
        self
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
        &self,
        mut map: impl FnMut(&D) -> Result<T, E>,
    ) -> Result<TestCaseOutcome<T>, E> {
        Ok(match self {
            Self::Passed => TestCaseOutcome::Passed,
            Self::Failed {
                diagnostic,
                related,
            } => TestCaseOutcome::Failed {
                diagnostic: map(diagnostic)?,
                related: related.iter().map(map).collect::<Result<Vec<_>, _>>()?,
            },
            Self::Error {
                diagnostic,
                related,
            } => TestCaseOutcome::Error {
                diagnostic: map(diagnostic)?,
                related: related.iter().map(map).collect::<Result<Vec<_>, _>>()?,
            },
            Self::Skipped { reason } => TestCaseOutcome::Skipped {
                reason: reason.clone(),
            },
        })
    }
}

pub type TestExecutionResult = TestCaseResult<Diagnostic>;
pub type TestExecutionOutcome = TestCaseOutcome<Diagnostic>;
pub type TestExecutionAttempt = TestCaseAttempt<Diagnostic>;
