use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Streams and structured logs captured during one test attempt.
pub struct CapturedTestOutput {
    stdout: String,
    stderr: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    logs: Vec<CapturedLog>,
}

impl CapturedTestOutput {
    /// Creates captured output without altering stream contents.
    pub fn new(stdout: String, stderr: String) -> Self {
        Self {
            stdout,
            stderr,
            logs: Vec::new(),
        }
    }

    /// Attaches logging records in emission order.
    #[must_use]
    pub fn with_logs(mut self, logs: Vec<CapturedLog>) -> Self {
        self.logs = logs;
        self
    }

    pub fn logs(&self) -> &[CapturedLog] {
        &self.logs
    }

    pub fn stdout(&self) -> &str {
        &self.stdout
    }

    pub fn stderr(&self) -> &str {
        &self.stderr
    }

    pub fn is_empty(&self) -> bool {
        self.stdout.is_empty() && self.stderr.is_empty() && self.logs.is_empty()
    }
}

/// Immutable logging event retained independently of the Python record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedLog {
    /// One-based attempt number, including worker-restart retries.
    pub attempt: u32,

    /// Lifecycle phase: setup, call, or teardown.
    pub phase: String,

    /// Decimal Unix timestamp in seconds, preserving Python's record precision.
    pub timestamp: String,

    pub logger: String,

    /// Python logging severity number.
    pub level: i64,

    pub level_name: String,

    /// Message after applying the record's arguments.
    pub message: String,

    pub pathname: String,

    /// One-based source line number.
    pub lineno: u32,

    /// Formatted exception traceback, when supplied by the application.
    pub exception: Option<String>,
}
