use crate::diagnostic::render::SubDiagnosticDisplay;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubDiagnostic {
    message: String,
    location: Option<String>,
    severity: SubDiagnosticSeverity,
}

impl SubDiagnostic {
    #[must_use]
    pub const fn new(
        message: String,
        location: Option<String>,
        severity: SubDiagnosticSeverity,
    ) -> Self {
        Self {
            message,
            location,
            severity,
        }
    }

    #[must_use]
    pub fn fixture_not_found(fixture_name: &String, location: Option<String>) -> Self {
        Self::new(
            format!("Fixture {fixture_name} not found"),
            location,
            SubDiagnosticSeverity::Error(SubDiagnosticErrorType::Fixture(
                FixtureSubDiagnosticType::NotFound,
            )),
        )
    }

    #[must_use]
    pub const fn display(&self) -> SubDiagnosticDisplay<'_> {
        SubDiagnosticDisplay::new(self)
    }

    #[must_use]
    pub const fn error_type(&self) -> Option<&SubDiagnosticErrorType> {
        match &self.severity {
            SubDiagnosticSeverity::Error(diagnostic_type) => Some(diagnostic_type),
            SubDiagnosticSeverity::Warning(_) => None,
        }
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub fn location(&self) -> Option<&str> {
        self.location.as_deref()
    }

    #[must_use]
    pub const fn severity(&self) -> &SubDiagnosticSeverity {
        &self.severity
    }
}

// Sub diagnostic severity
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubDiagnosticSeverity {
    Error(SubDiagnosticErrorType),
    Warning(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubDiagnosticErrorType {
    Fixture(FixtureSubDiagnosticType),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FixtureSubDiagnosticType {
    NotFound,
}
