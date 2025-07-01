use karva_project::path::TestPathError;
use pyo3::prelude::*;

use crate::{
    diagnostic::render::{DisplayDiagnostic, SubDiagnosticDisplay},
    models::TestFunction,
};

pub mod render;
pub mod reporter;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    sub_diagnostics: Vec<SubDiagnostic>,
    scope: DiagnosticScope,
}

impl Diagnostic {
    const fn new(sub_diagnostics: Vec<SubDiagnostic>, scope: DiagnosticScope) -> Self {
        Self {
            sub_diagnostics,
            scope,
        }
    }

    #[must_use]
    pub fn sub_diagnostics(&self) -> &[SubDiagnostic] {
        &self.sub_diagnostics
    }

    #[must_use]
    pub const fn scope(&self) -> &DiagnosticScope {
        &self.scope
    }

    pub fn from_py_err(
        py: Python<'_>,
        error: &PyErr,
        scope: DiagnosticScope,
        location: Option<String>,
        severity: Severity,
    ) -> Self {
        Self::new(
            vec![SubDiagnostic::new(
                get_traceback(py, error),
                location,
                severity,
            )],
            scope,
        )
    }

    pub fn from_test_fail(py: Python<'_>, error: &PyErr, test_case: &TestFunction) -> Self {
        if error.is_instance_of::<pyo3::exceptions::PyAssertionError>(py) {
            return Self::new(
                vec![SubDiagnostic::new(
                    get_traceback(py, error),
                    Some(test_case.path().to_string()),
                    Severity::Error(ErrorType::TestCase(TestCaseDiagnosticType::Fail)),
                )],
                DiagnosticScope::Test,
            );
        }
        Self::from_py_err(
            py,
            error,
            DiagnosticScope::Test,
            Some(test_case.path().to_string()),
            Severity::Error(ErrorType::TestCase(TestCaseDiagnosticType::Error(
                get_type_name(py, error),
            ))),
        )
    }

    #[must_use]
    pub fn fixture_not_found(fixture_name: &String, location: Option<String>) -> Self {
        Self::new(
            vec![SubDiagnostic::new(
                format!("Fixture {fixture_name} not found"),
                location,
                Severity::Error(ErrorType::Fixture("fixture-not-found".to_string())),
            )],
            DiagnosticScope::Setup,
        )
    }

    #[must_use]
    pub fn invalid_fixture(message: String, location: Option<String>) -> Self {
        Self::new(
            vec![SubDiagnostic::new(
                message,
                location,
                Severity::Error(ErrorType::Fixture("invalid-fixture".to_string())),
            )],
            DiagnosticScope::Setup,
        )
    }

    #[must_use]
    pub fn invalid_path_error(error: &TestPathError) -> Self {
        let path = error.path().to_string();
        Self::new(
            vec![SubDiagnostic::new(
                format!("{error}"),
                Some(path),
                Severity::Error(ErrorType::Known("invalid-path".to_string())),
            )],
            DiagnosticScope::Unknown,
        )
    }

    #[must_use]
    pub fn unknown_error(message: String, location: Option<String>) -> Self {
        Self::new(
            vec![SubDiagnostic::new(
                message,
                location,
                Severity::Error(ErrorType::Unknown),
            )],
            DiagnosticScope::Unknown,
        )
    }

    #[must_use]
    pub fn warning(
        warning_type: &str,
        message: String,
        location: Option<String>,
        scope: DiagnosticScope,
    ) -> Self {
        Self::new(
            vec![SubDiagnostic::new(
                message,
                location,
                Severity::Warning(warning_type.to_string()),
            )],
            scope,
        )
    }

    #[must_use]
    pub const fn from_sub_diagnostics(
        sub_diagnostics: Vec<SubDiagnostic>,
        scope: DiagnosticScope,
    ) -> Self {
        Self::new(sub_diagnostics, scope)
    }

    #[must_use]
    pub fn from_test_diagnostics(diagnostic: Vec<Self>) -> Self {
        let mut sub_diagnostics = Vec::new();
        for diagnostic in diagnostic {
            sub_diagnostics.extend(diagnostic.sub_diagnostics);
        }
        Self::new(sub_diagnostics, DiagnosticScope::Test)
    }

    pub fn add_sub_diagnostic(&mut self, sub_diagnostic: SubDiagnostic) {
        self.sub_diagnostics.push(sub_diagnostic);
    }

    #[must_use]
    pub fn severity(&self) -> Severity {
        self.sub_diagnostics
            .iter()
            .map(SubDiagnostic::severity)
            .find(|severity| severity.is_error())
            .or_else(|| {
                self.sub_diagnostics
                    .iter()
                    .map(SubDiagnostic::severity)
                    .find(|severity| matches!(severity, Severity::Warning(_)))
            })
            .map_or_else(|| Severity::Warning("unknown".to_string()), Clone::clone)
    }

    #[must_use]
    pub const fn display(&self) -> DisplayDiagnostic<'_> {
        DisplayDiagnostic::new(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubDiagnostic {
    message: String,
    location: Option<String>,
    severity: Severity,
}

impl SubDiagnostic {
    #[must_use]
    pub const fn new(message: String, location: Option<String>, severity: Severity) -> Self {
        Self {
            message,
            location,
            severity,
        }
    }
    #[must_use]
    pub const fn display(&self) -> SubDiagnosticDisplay<'_> {
        SubDiagnosticDisplay::new(self)
    }

    #[must_use]
    pub const fn error_type(&self) -> Option<&ErrorType> {
        match &self.severity {
            Severity::Error(diagnostic_type) => Some(diagnostic_type),
            Severity::Warning(_) => None,
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
    pub const fn severity(&self) -> &Severity {
        &self.severity
    }
}

// Where the diagnostic is coming from
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticScope {
    Test,
    Setup,
    Discovery,
    Teardown,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error(ErrorType),
    Warning(String),
}

impl Severity {
    #[must_use]
    pub const fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }

    #[must_use]
    pub const fn is_test_fail(&self) -> bool {
        matches!(
            self,
            Self::Error(ErrorType::TestCase(TestCaseDiagnosticType::Fail))
        )
    }
    #[must_use]
    pub const fn is_test_error(&self) -> bool {
        matches!(
            self,
            Self::Error(ErrorType::TestCase(TestCaseDiagnosticType::Error(_)))
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorType {
    TestCase(TestCaseDiagnosticType),
    Fixture(String),
    Known(String),
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestCaseDiagnosticType {
    Fail,
    Error(String),
}

fn get_traceback(py: Python<'_>, error: &PyErr) -> String {
    if let Some(traceback) = error.traceback(py) {
        let traceback_str = traceback.format().unwrap_or_default();
        if traceback_str.is_empty() {
            return error.to_string();
        }
        filter_traceback(&traceback_str)
    } else {
        error.to_string()
    }
}

fn get_type_name(py: Python<'_>, error: &PyErr) -> String {
    error
        .get_type(py)
        .name()
        .map_or_else(|_| "Unknown".to_string(), |name| name.to_string())
}

// Simplified traceback filtering that removes unnecessary traceback headers
fn filter_traceback(traceback: &str) -> String {
    let lines: Vec<&str> = traceback.lines().collect();
    let mut filtered = String::new();

    for (i, line) in lines.iter().enumerate() {
        if i == 0 && line.contains("Traceback (most recent call last):") {
            continue;
        }
        filtered.push_str(line.strip_prefix("  ").unwrap_or(line));
        filtered.push('\n');
    }
    filtered = filtered.trim_end_matches('\n').to_string();

    filtered = filtered.trim_end_matches('^').to_string();

    filtered.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use pyo3::exceptions::{PyAssertionError, PyTypeError};

    use super::*;

    #[test]
    fn test_get_type_name() {
        Python::with_gil(|py| {
            let error = PyTypeError::new_err("Error message");
            let type_name = get_type_name(py, &error);
            assert_eq!(type_name, "TypeError");
        });
    }

    #[test]
    fn test_from_sub_diagnostics() {
        let sub_diagnostic = SubDiagnostic::new(
            "message".to_string(),
            None,
            Severity::Error(ErrorType::TestCase(TestCaseDiagnosticType::Fail)),
        );
        let diagnostic =
            Diagnostic::from_sub_diagnostics(vec![sub_diagnostic.clone()], DiagnosticScope::Test);
        assert_eq!(diagnostic.sub_diagnostics(), &[sub_diagnostic]);
    }

    #[test]
    fn test_from_test_diagnostics() {
        let sub_diagnostic = SubDiagnostic::new(
            "message".to_string(),
            None,
            Severity::Error(ErrorType::TestCase(TestCaseDiagnosticType::Fail)),
        );
        let diagnostic = Diagnostic::from_test_diagnostics(vec![Diagnostic::from_sub_diagnostics(
            vec![sub_diagnostic.clone()],
            DiagnosticScope::Unknown,
        )]);
        assert_eq!(diagnostic.sub_diagnostics(), &[sub_diagnostic]);
        assert_eq!(diagnostic.scope(), &DiagnosticScope::Test);
    }

    #[test]
    fn test_add_sub_diagnostic() {
        let mut diagnostic = Diagnostic::new(vec![], DiagnosticScope::Test);
        let sub_diagnostic = SubDiagnostic::new(
            "message".to_string(),
            None,
            Severity::Error(ErrorType::TestCase(TestCaseDiagnosticType::Fail)),
        );
        diagnostic.add_sub_diagnostic(sub_diagnostic.clone());
        assert_eq!(diagnostic.sub_diagnostics(), &[sub_diagnostic]);
    }

    #[test]
    fn test_subdiagnostic() {
        let sub_diagnostic = SubDiagnostic::new(
            "message".to_string(),
            None,
            Severity::Error(ErrorType::TestCase(TestCaseDiagnosticType::Fail)),
        );
        assert_eq!(
            sub_diagnostic.error_type(),
            Some(&ErrorType::TestCase(TestCaseDiagnosticType::Fail))
        );
        assert_eq!(sub_diagnostic.message(), "message");
        assert_eq!(sub_diagnostic.location(), None);
        assert_eq!(
            sub_diagnostic.severity(),
            &Severity::Error(ErrorType::TestCase(TestCaseDiagnosticType::Fail))
        );
    }

    #[test]
    fn test_get_traceback() {
        Python::with_gil(|py| {
            let error = PyAssertionError::new_err("This is an error");
            let traceback = get_traceback(py, &error);
            assert_eq!(traceback, "AssertionError: This is an error");
        });
    }

    #[test]
    fn test_get_traceback_empty() {
        Python::with_gil(|py| {
            let error = PyAssertionError::new_err("");
            let traceback = get_traceback(py, &error);
            assert_eq!(traceback, "AssertionError: ");
        });
    }
}
