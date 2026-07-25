use std::time::Duration;

use karva_python_semantic::QualifiedTestName;
use ruff_db::diagnostic::{Diagnostic, Severity};
use serde::{Deserialize, Serialize};

use super::kind::IndividualTestResultKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCaseResult<D = RenderedDiagnostic> {
    module_name: String,
    name: String,
    full_name: String,
    outcome: TestCaseOutcome<D>,
    duration: Duration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry: Option<TestCaseRetry>,
}

impl<D> TestCaseResult<D> {
    pub fn new(
        test_case_name: &QualifiedTestName,
        outcome: TestCaseOutcome<D>,
        duration: Duration,
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
        }
    }

    pub fn retried(
        test_case_name: &QualifiedTestName,
        outcome: TestCaseOutcome<D>,
        duration: Duration,
        retry: TestCaseRetry,
    ) -> Self {
        let mut result = Self::new(test_case_name, outcome, duration);
        result.retry = Some(retry);
        result
    }

    pub fn from_display_name(
        full_name: &str,
        outcome: TestCaseOutcome<D>,
        duration: Duration,
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

    pub fn try_map_diagnostic<T, E>(
        &self,
        map: impl FnOnce(&D) -> Result<T, E>,
    ) -> Result<TestCaseResult<T>, E> {
        Ok(TestCaseResult {
            module_name: self.module_name.clone(),
            name: self.name.clone(),
            full_name: self.full_name.clone(),
            outcome: self.outcome.try_map_diagnostic(map)?,
            duration: self.duration,
            retry: self.retry.clone(),
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
    Failed { diagnostic: D },
    Skipped { reason: Option<String> },
}

impl<D> TestCaseOutcome<D> {
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    pub fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped { .. })
    }

    pub fn diagnostic(&self) -> Option<&D> {
        match self {
            Self::Failed { diagnostic } => Some(diagnostic),
            Self::Passed | Self::Skipped { .. } => None,
        }
    }

    pub fn result_kind(&self) -> IndividualTestResultKind {
        match self {
            Self::Passed => IndividualTestResultKind::Passed,
            Self::Failed { .. } => IndividualTestResultKind::Failed,
            Self::Skipped { reason } => IndividualTestResultKind::Skipped {
                reason: reason.clone(),
            },
        }
    }

    fn try_map_diagnostic<T, E>(
        &self,
        map: impl FnOnce(&D) -> Result<T, E>,
    ) -> Result<TestCaseOutcome<T>, E> {
        Ok(match self {
            Self::Passed => TestCaseOutcome::Passed,
            Self::Failed { diagnostic } => TestCaseOutcome::Failed {
                diagnostic: map(diagnostic)?,
            },
            Self::Skipped { reason } => TestCaseOutcome::Skipped {
                reason: reason.clone(),
            },
        })
    }
}

pub type TestExecutionResult = TestCaseResult<Diagnostic>;
pub type TestExecutionOutcome = TestCaseOutcome<Diagnostic>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedDiagnostic {
    code: String,
    severity: DiagnosticSeverity,
    message: String,
    rendered: String,
}

impl RenderedDiagnostic {
    pub fn new(
        code: impl Into<String>,
        severity: DiagnosticSeverity,
        message: impl Into<String>,
        rendered: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity,
            message: message.into(),
            rendered: rendered.into(),
        }
    }

    pub fn interrupted(test_name: &str) -> Self {
        let message = format!("Test `{test_name}` was interrupted");
        Self::new(
            "interrupted",
            DiagnosticSeverity::Error,
            &message,
            format!("error[interrupted]: {message}\n"),
        )
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn severity(&self) -> DiagnosticSeverity {
        self.severity
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn rendered(&self) -> &str {
        &self.rendered
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
    Fatal,
}

impl From<Severity> for DiagnosticSeverity {
    fn from(value: Severity) -> Self {
        match value {
            Severity::Info => Self::Info,
            Severity::Warning => Self::Warning,
            Severity::Error => Self::Error,
            Severity::Fatal => Self::Fatal,
        }
    }
}
