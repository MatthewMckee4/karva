use crate::diagnostic::render::SubDiagnosticDisplay;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SubDiagnostic {
    message: String,
    severity: SubDiagnosticSeverity,
}

impl SubDiagnostic {
    #[must_use]
    pub(crate) const fn new(message: String, severity: SubDiagnosticSeverity) -> Self {
        Self { message, severity }
    }

    #[must_use]
    pub(crate) fn fixture_not_found(fixture_name: &String) -> Self {
        Self::new(
            format!("fixture '{fixture_name}' not found"),
            SubDiagnosticSeverity::Error(SubDiagnosticErrorType::Fixture(
                FixtureSubDiagnosticType::NotFound(fixture_name.clone()),
            )),
        )
    }

    #[must_use]
    pub(crate) const fn display(&self) -> SubDiagnosticDisplay<'_> {
        SubDiagnosticDisplay::new(self)
    }

    #[must_use]
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub(crate) const fn severity(&self) -> &SubDiagnosticSeverity {
        &self.severity
    }
}

// Sub diagnostic severity
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SubDiagnosticSeverity {
    Error(SubDiagnosticErrorType),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SubDiagnosticErrorType {
    Fixture(FixtureSubDiagnosticType),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FixtureSubDiagnosticType {
    NotFound(String),
}
