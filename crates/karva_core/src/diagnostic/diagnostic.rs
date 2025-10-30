use karva_project::path::TestPathError;
use pyo3::prelude::*;

use crate::{
    collection::TestCase,
    diagnostic::{
        render::{DiagnosticInnerDisplay, DisplayDiagnostic},
        sub_diagnostic::SubDiagnostic,
        utils::{get_traceback, get_type_name, to_kebab_case},
    },
    discovery::DiscoveredModule,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    inner: DiagnosticInner,
    sub_diagnostics: Vec<SubDiagnostic>,
}

impl Diagnostic {
    pub(crate) const fn new(
        message: Option<String>,
        location: Option<String>,
        traceback: Option<String>,
        severity: DiagnosticSeverity,
    ) -> Self {
        Self {
            inner: DiagnosticInner {
                message,
                location,
                traceback,
                severity,
            },
            sub_diagnostics: Vec::new(),
        }
    }

    pub(crate) fn clear_sub_diagnostics(&mut self) {
        self.sub_diagnostics.clear();
    }

    pub(crate) fn add_sub_diagnostics(&mut self, sub_diagnostics: Vec<SubDiagnostic>) {
        self.sub_diagnostics.extend(sub_diagnostics);
    }

    pub(crate) fn sub_diagnostics(&self) -> &[SubDiagnostic] {
        &self.sub_diagnostics
    }

    pub(crate) const fn severity(&self) -> &DiagnosticSeverity {
        &self.inner.severity
    }

    pub const fn display(&self) -> DisplayDiagnostic<'_> {
        DisplayDiagnostic::new(self)
    }

    pub(crate) const fn inner(&self) -> &DiagnosticInner {
        &self.inner
    }

    pub(crate) fn from_test_fail(
        py: Python<'_>,
        error: &PyErr,
        test_case: &TestCase,
        module: &DiscoveredModule,
    ) -> Self {
        let message = {
            let msg = error.value(py).to_string();
            if msg.is_empty() { None } else { Some(msg) }
        };
        Self::new(
            message,
            Some(test_case.function().display_with_line(module)),
            Some(get_traceback(py, error)),
            DiagnosticSeverity::Error(DiagnosticErrorType::TestCase {
                test_name: test_case.function().name().to_string(),
                diagnostic_type: TestCaseDiagnosticType::Fail(to_kebab_case(&get_type_name(
                    py, error,
                ))),
            }),
        )
    }

    pub(crate) fn invalid_path_error(error: &TestPathError) -> Self {
        Self::new(
            Some(format!("{error}")),
            None,
            None,
            DiagnosticSeverity::Error(DiagnosticErrorType::Known("invalid-path".to_string())),
        )
    }

    pub(crate) fn warning(
        warning_type: &str,
        message: Option<String>,
        location: Option<String>,
    ) -> Self {
        Self::new(
            message,
            location,
            None,
            DiagnosticSeverity::Warning(warning_type.to_string()),
        )
    }

    pub(crate) const fn invalid_fixture(message: Option<String>, location: Option<String>) -> Self {
        Self::new(
            message,
            location,
            None,
            DiagnosticSeverity::Error(DiagnosticErrorType::Fixture(FixtureDiagnosticType::Invalid)),
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiagnosticInner {
    message: Option<String>,
    location: Option<String>,
    traceback: Option<String>,
    severity: DiagnosticSeverity,
}

impl DiagnosticInner {
    #[cfg(test)]
    pub(crate) const fn new(
        message: Option<String>,
        location: Option<String>,
        traceback: Option<String>,
        severity: DiagnosticSeverity,
    ) -> Self {
        Self {
            message,
            location,
            traceback,
            severity,
        }
    }

    pub(crate) const fn display(&self) -> DiagnosticInnerDisplay<'_> {
        DiagnosticInnerDisplay::new(self)
    }

    pub(crate) fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    pub(crate) fn location(&self) -> Option<&str> {
        self.location.as_deref()
    }

    pub(crate) fn traceback(&self) -> Option<&str> {
        self.traceback.as_deref()
    }

    pub(crate) const fn severity(&self) -> &DiagnosticSeverity {
        &self.severity
    }
}

// Diagnostic severity
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiagnosticSeverity {
    Error(DiagnosticErrorType),
    Warning(String),
}

impl DiagnosticSeverity {
    pub(crate) const fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }

    pub(crate) const fn is_test_fail(&self) -> bool {
        matches!(self, Self::Error(DiagnosticErrorType::TestCase { .. }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DiagnosticErrorType {
    TestCase {
        test_name: String,
        diagnostic_type: TestCaseDiagnosticType,
    },
    Fixture(FixtureDiagnosticType),
    Known(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TestCaseDiagnosticType {
    Fail(String),
    Collection(TestCaseCollectionDiagnosticType),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TestCaseCollectionDiagnosticType {
    FixtureNotFound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FixtureDiagnosticType {
    Invalid,
}
