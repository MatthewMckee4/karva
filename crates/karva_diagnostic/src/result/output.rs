use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapturedTestOutput {
    stdout: String,
    stderr: String,
}

impl CapturedTestOutput {
    pub fn new(stdout: String, stderr: String) -> Self {
        Self { stdout, stderr }
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
