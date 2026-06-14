use pyo3::prelude::*;

use super::parse_pytest_mark_args;

/// Represents a test marked as expected to fail (xfail).
///
/// If the test fails, it counts as passed (expected failure).
/// If the test passes unexpectedly, it counts as failed.
/// Supports conditional xfail via boolean conditions.
#[derive(Debug, Clone)]
pub struct ExpectFailTag {
    /// Boolean conditions; test is xfailed if any is true (or if empty).
    conditions: Vec<bool>,

    /// Optional explanation for why failure is expected.
    reason: Option<String>,
}

impl ExpectFailTag {
    pub(crate) fn new(conditions: Vec<bool>, reason: Option<String>) -> Self {
        Self { conditions, reason }
    }

    pub(crate) fn reason(&self) -> Option<String> {
        self.reason.clone()
    }

    /// Check if the test should be expected to fail.
    /// If there are no conditions, always expect fail.
    /// If there are conditions, expect fail only if any condition is true.
    pub(crate) fn should_expect_fail(&self) -> bool {
        if self.conditions.is_empty() {
            true
        } else {
            self.conditions.iter().any(|&c| c)
        }
    }

    pub(crate) fn try_from_pytest_mark(py_mark: &Bound<'_, PyAny>) -> PyResult<Option<Self>> {
        let parsed = parse_pytest_mark_args(py_mark)?;
        Ok(Some(Self {
            conditions: parsed.conditions,
            reason: parsed.reason,
        }))
    }
}
