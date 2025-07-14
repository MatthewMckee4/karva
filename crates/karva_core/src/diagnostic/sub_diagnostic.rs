use crate::diagnostic::render::SubDiagnosticDisplay;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubDiagnostic {
    message: String,
    severity: SubDiagnosticSeverity,
}

impl SubDiagnostic {
    #[must_use]
    pub const fn new(message: String, severity: SubDiagnosticSeverity) -> Self {
        Self { message, severity }
    }

    #[must_use]
    pub fn fixture_not_found(fixture_name: &String) -> Self {
        Self::new(
            format!("fixture '{fixture_name}' not found"),
            SubDiagnosticSeverity::Error(SubDiagnosticErrorType::Fixture(
                FixtureSubDiagnosticType::NotFound(fixture_name.clone()),
            )),
        )
    }

    #[must_use]
    pub const fn display(&self) -> SubDiagnosticDisplay<'_> {
        SubDiagnosticDisplay::new(self)
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
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
    NotFound(String),
}
