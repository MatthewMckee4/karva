use serde::{Deserialize, Serialize};

use crate::Severity;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Diagnostic serialized for controller-side display after worker execution.
pub struct RenderedDiagnostic {
    code: String,
    severity: Severity,
    message: String,
    rendered: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    colored_rendered: Option<String>,
}

impl RenderedDiagnostic {
    /// Creates a diagnostic with plain rendering and no terminal-specific variant.
    pub(crate) fn new(
        code: impl Into<String>,
        severity: Severity,
        message: impl Into<String>,
        rendered: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            severity,
            message: message.into(),
            rendered: rendered.into(),
            colored_rendered: None,
        }
    }

    /// Stores a colored rendering only when it differs from plain output.
    #[must_use]
    pub(crate) fn with_colored_rendered(mut self, rendered: String) -> Self {
        if rendered != self.rendered {
            self.colored_rendered = Some(rendered);
        }
        self
    }

    /// Creates the synthetic diagnostic used when shutdown interrupts a test.
    pub(super) fn interrupted(test_name: &str) -> Self {
        let message = format!("Test `{test_name}` was interrupted");
        Self::new(
            "interrupted",
            Severity::Error,
            &message,
            format!("error[interrupted]: {message}\n"),
        )
    }

    /// Creates a synthetic diagnostic for an unexpectedly terminated worker.
    pub fn worker_crashed(test_name: &str, termination: &str, stderr: &str) -> Self {
        let message = format!("Worker terminated with {termination} while running `{test_name}`");
        let mut rendered = format!("error[worker-crashed]: {message}\n");
        if !stderr.trim().is_empty() {
            rendered.push_str("\nWorker stderr:\n");
            rendered.push_str(stderr.trim_end());
            rendered.push('\n');
        }
        Self::new("worker-crashed", Severity::Error, &message, rendered)
    }

    /// Creates a run-level diagnostic when no active test can be identified.
    pub fn worker_exited(worker_id: usize, termination: &str, stderr: &str) -> Self {
        let message = format!("Worker {worker_id} terminated with {termination}");
        let mut rendered = format!("error[worker-crashed]: {message}\n");
        if !stderr.trim().is_empty() {
            rendered.push_str("\nWorker stderr:\n");
            rendered.push_str(stderr.trim_end());
            rendered.push('\n');
        }
        Self::new("worker-crashed", Severity::Error, &message, rendered)
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn severity_name(&self) -> &'static str {
        match self.severity {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Fatal => "fatal",
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self.severity, Severity::Error | Severity::Fatal)
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn rendered(&self) -> &str {
        &self.rendered
    }

    /// Returns rendering appropriate for a color-capable terminal.
    pub fn rendered_for_terminal(&self) -> &str {
        self.colored_rendered.as_deref().unwrap_or(&self.rendered)
    }
}
