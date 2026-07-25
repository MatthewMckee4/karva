use ruff_db::diagnostic::Severity;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderedDiagnostic {
    code: String,
    #[serde(with = "SerializableSeverity")]
    severity: Severity,
    message: String,
    rendered: String,
}

impl RenderedDiagnostic {
    pub fn new(
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
        }
    }

    pub fn interrupted(test_name: &str) -> Self {
        let message = format!("Test `{test_name}` was interrupted");
        Self::new(
            "interrupted",
            Severity::Error,
            &message,
            format!("error[interrupted]: {message}\n"),
        )
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn severity(&self) -> Severity {
        self.severity
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn rendered(&self) -> &str {
        &self.rendered
    }
}

#[derive(Serialize, Deserialize)]
#[serde(remote = "Severity", rename_all = "lowercase")]
enum SerializableSeverity {
    Info,
    Warning,
    Error,
    Fatal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_ruff_severity() {
        let diagnostic =
            RenderedDiagnostic::new("test-failure", Severity::Error, "failed", "rendered");

        let json = serde_json::to_string(&diagnostic).unwrap();
        let roundtrip: RenderedDiagnostic = serde_json::from_str(&json).unwrap();

        assert_eq!(roundtrip, diagnostic);
        assert!(json.contains(r#""severity":"error""#));
    }
}
