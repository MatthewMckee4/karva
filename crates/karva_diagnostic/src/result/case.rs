use std::time::Duration;

use karva_python_semantic::QualifiedTestName;
use serde::{Deserialize, Serialize};

use super::kind::IndividualTestResultKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TestCaseResult {
    module_name: String,
    name: String,
    full_name: String,
    outcome: TestCaseOutcome,
    duration: Duration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    retry: Option<TestCaseRetry>,
}

impl TestCaseResult {
    pub fn new(
        test_case_name: &QualifiedTestName,
        outcome: TestCaseOutcome,
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
        outcome: TestCaseOutcome,
        duration: Duration,
        retry: TestCaseRetry,
    ) -> Self {
        let mut result = Self::new(test_case_name, outcome, duration);
        result.retry = Some(retry);
        result
    }

    pub fn from_display_name(
        full_name: &str,
        outcome: TestCaseOutcome,
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

    pub fn outcome(&self) -> &TestCaseOutcome {
        &self.outcome
    }

    pub fn duration(&self) -> Duration {
        self.duration
    }

    pub fn retry(&self) -> Option<&TestCaseRetry> {
        self.retry.as_ref()
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
pub enum TestCaseOutcome {
    Passed,
    Failed,
    Skipped { reason: Option<String> },
}

impl TestCaseOutcome {
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed)
    }

    pub fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped { .. })
    }
}

impl From<&IndividualTestResultKind> for TestCaseOutcome {
    fn from(value: &IndividualTestResultKind) -> Self {
        match value {
            IndividualTestResultKind::Passed => Self::Passed,
            IndividualTestResultKind::Failed => Self::Failed,
            IndividualTestResultKind::Skipped { reason } => Self::Skipped {
                reason: reason.clone(),
            },
        }
    }
}
