use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::kind::IndividualTestResultKind;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedTestOutput {
    test_name: String,
    outcome: CapturedTestOutcome,
    stdout: String,
    stderr: String,
}

pub fn captured_outputs_by_test(
    outputs: &[CapturedTestOutput],
) -> HashMap<&str, &CapturedTestOutput> {
    outputs
        .iter()
        .map(|output| (output.test_name(), output))
        .collect()
}

impl CapturedTestOutput {
    pub fn new(
        test_name: String,
        outcome: CapturedTestOutcome,
        stdout: String,
        stderr: String,
    ) -> Self {
        Self {
            test_name,
            outcome,
            stdout,
            stderr,
        }
    }

    pub fn test_name(&self) -> &str {
        &self.test_name
    }

    pub fn outcome(&self) -> CapturedTestOutcome {
        self.outcome
    }

    pub fn stdout(&self) -> &str {
        &self.stdout
    }

    pub fn stderr(&self) -> &str {
        &self.stderr
    }

    pub fn is_empty(&self) -> bool {
        self.stdout.is_empty() && self.stderr.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapturedTestOutcome {
    Passed,
    Failed,
    Skipped,
}

impl CapturedTestOutcome {
    pub fn is_failed(self) -> bool {
        matches!(self, Self::Failed)
    }
}

impl From<&IndividualTestResultKind> for CapturedTestOutcome {
    fn from(value: &IndividualTestResultKind) -> Self {
        match value {
            IndividualTestResultKind::Passed => Self::Passed,
            IndividualTestResultKind::Failed => Self::Failed,
            IndividualTestResultKind::Skipped { .. } => Self::Skipped,
        }
    }
}
