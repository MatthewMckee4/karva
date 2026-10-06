use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Stdout and stderr captured during one test attempt.
pub struct CapturedTestOutput {
    stdout: String,
    stderr: String,
    #[serde(skip)]
    raw_stdout: Vec<u8>,
    #[serde(skip)]
    raw_stderr: Vec<u8>,
    #[serde(skip)]
    stdout_total: u64,
    #[serde(skip)]
    stderr_total: u64,
}

impl CapturedTestOutput {
    /// Creates captured output without altering stream contents.
    #[expect(clippy::needless_pass_by_value)]
    pub fn new(stdout: String, stderr: String) -> Self {
        let stdout_total = stdout.len() as u64;
        let stderr_total = stderr.len() as u64;
        Self::with_totals(&stdout, &stderr, stdout_total, stderr_total)
    }

    /// Creates captured output while retaining the raw byte totals used by
    /// aggregate truncation diagnostics.
    pub fn with_totals(stdout: &str, stderr: &str, stdout_total: u64, stderr_total: u64) -> Self {
        Self::with_raw_totals(
            stdout.to_owned(),
            stderr.to_owned(),
            stdout.as_bytes().to_vec(),
            stderr.as_bytes().to_vec(),
            stdout_total,
            stderr_total,
        )
    }

    /// Creates rendered output while retaining the bounded raw stream parts
    /// used to combine retries without nesting truncation markers.
    pub fn with_raw_totals(
        stdout: String,
        stderr: String,
        raw_stdout: Vec<u8>,
        raw_stderr: Vec<u8>,
        stdout_total: u64,
        stderr_total: u64,
    ) -> Self {
        Self {
            stdout,
            stderr,
            raw_stdout,
            raw_stderr,
            stdout_total,
            stderr_total,
        }
    }

    pub fn stdout(&self) -> &str {
        &self.stdout
    }

    pub fn stderr(&self) -> &str {
        &self.stderr
    }

    /// Returns bounded raw stdout bytes for aggregate retention.
    pub fn raw_stdout(&self) -> &[u8] {
        &self.raw_stdout
    }

    /// Returns bounded raw stderr bytes for aggregate retention.
    pub fn raw_stderr(&self) -> &[u8] {
        &self.raw_stderr
    }

    pub fn stdout_total(&self) -> u64 {
        self.stdout_total
    }

    pub fn stderr_total(&self) -> u64 {
        self.stderr_total
    }

    pub fn is_empty(&self) -> bool {
        self.stdout.is_empty() && self.stderr.is_empty()
    }
}
