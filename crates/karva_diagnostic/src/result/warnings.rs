//! Transport-safe warning observations with test and retry attribution.

use serde::{Deserialize, Serialize};

/// One warning emitted after Python filtering and user recording contexts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TestWarning {
    /// Warning category name reported by Python.
    pub category: String,

    /// Warning text, without terminal formatting.
    pub message: String,

    /// Project-relative source path when possible.
    pub filename: String,

    /// One-based source line reported by Python.
    pub lineno: u32,

    /// Active test identity, absent during collection and broader-scope cleanup.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<String>,

    /// One-based attempt number, absent for run-level warnings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<u32>,
}
