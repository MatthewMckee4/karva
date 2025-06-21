use karva_project::path::TestPathError;
use pyo3::prelude::*;

use crate::{
    case::TestFunction,
    diagnostic::render::{DisplayDiagnostic, SubDiagnosticDisplay},
};

pub mod render;
pub mod reporter;

#[derive(Clone, Debug)]
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
        location: &str,
    ) -> Self {
        Self::new(
            vec![SubDiagnostic {
                diagnostic_type: SubDiagnosticType::Error(DiagnosticError::Error(get_type_name(
                    py, error,
                ))),
                message: get_traceback(py, error),
                location: location.to_string(),
            }],
            scope,
        )
    }

    pub fn from_test_fail(py: Python<'_>, error: &PyErr, test_case: &TestFunction) -> Self {
        if error.is_instance_of::<pyo3::exceptions::PyAssertionError>(py) {
            return Self::new(
                vec![SubDiagnostic {
                    diagnostic_type: SubDiagnosticType::Fail,
                    message: get_traceback(py, error),
                    location: test_case.path().to_string(),
                }],
                DiagnosticScope::Test,
            );
        }
        Self::from_py_err(
            py,
            error,
            DiagnosticScope::Test,
            &test_case.path().to_string(),
        )
    }

    #[must_use]
    pub fn fixture_not_found(fixture_name: &str, location: &str) -> Self {
        Self::new(
            vec![SubDiagnostic {
                diagnostic_type: SubDiagnosticType::Error(DiagnosticError::FixtureNotFound(
                    fixture_name.to_string(),
                )),
                message: format!("Fixture {fixture_name} not found"),
                location: location.to_string(),
            }],
            DiagnosticScope::Setup,
        )
    }

    #[must_use]
    pub fn invalid_fixture(message: &str, location: &str) -> Self {
        Self::new(
            vec![SubDiagnostic {
                diagnostic_type: SubDiagnosticType::Error(DiagnosticError::InvalidFixture(
                    message.to_string(),
                )),
                message: message.to_string(),
                location: location.to_string(),
            }],
            DiagnosticScope::Setup,
        )
    }

    #[must_use]
    pub fn path_error(error: &TestPathError) -> Self {
        Self::new(
            vec![SubDiagnostic {
                diagnostic_type: SubDiagnosticType::Error(DiagnosticError::InvalidPath(
                    error.path().to_string(),
                )),
                message: format!("{error}"),
                location: "setup".to_string(),
            }],
            DiagnosticScope::Unknown,
        )
    }

    #[must_use]
    pub fn unknown_error(message: &str, location: &str) -> Self {
        Self::new(
            vec![SubDiagnostic {
                diagnostic_type: SubDiagnosticType::Error(DiagnosticError::Error(
                    message.to_string(),
                )),
                message: message.to_string(),
                location: location.to_string(),
            }],
            DiagnosticScope::Unknown,
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
    pub fn diagnostic_type(&self) -> SubDiagnosticType {
        self.sub_diagnostics
            .iter()
            .map(|sub_diagnostic| sub_diagnostic.diagnostic_type().clone())
            .find(|diagnostic_type| matches!(diagnostic_type, SubDiagnosticType::Error(_)))
            .unwrap_or(SubDiagnosticType::Fail)
    }

    #[must_use]
    pub const fn display(&self) -> DisplayDiagnostic {
        DisplayDiagnostic::new(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubDiagnostic {
    diagnostic_type: SubDiagnosticType,
    message: String,
    location: String,
}

impl SubDiagnostic {
    #[must_use]
    pub const fn new(
        diagnostic_type: SubDiagnosticType,
        message: String,
        location: String,
    ) -> Self {
        Self {
            diagnostic_type,
            message,
            location,
        }
    }
    #[must_use]
    pub const fn display(&self) -> SubDiagnosticDisplay {
        SubDiagnosticDisplay::new(self)
    }

    #[must_use]
    pub const fn diagnostic_type(&self) -> &SubDiagnosticType {
        &self.diagnostic_type
    }

    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[must_use]
    pub fn location(&self) -> &str {
        &self.location
    }
}

// Where the diagnostic is coming from
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticScope {
    Test,
    Setup,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubDiagnosticType {
    Fail,
    Error(DiagnosticError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticError {
    Error(String),
    FixtureNotFound(String),
    InvalidFixture(String),
    InvalidPath(String),
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
            SubDiagnosticType::Fail,
            "message".to_string(),
            "location".to_string(),
        );
        let diagnostic =
            Diagnostic::from_sub_diagnostics(vec![sub_diagnostic.clone()], DiagnosticScope::Test);
        assert_eq!(diagnostic.sub_diagnostics(), &[sub_diagnostic]);
    }

    #[test]
    fn test_from_test_diagnostics() {
        let sub_diagnostic = SubDiagnostic::new(
            SubDiagnosticType::Fail,
            "message".to_string(),
            "location".to_string(),
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
            SubDiagnosticType::Fail,
            "message".to_string(),
            "location".to_string(),
        );
        diagnostic.add_sub_diagnostic(sub_diagnostic.clone());
        assert_eq!(diagnostic.sub_diagnostics(), &[sub_diagnostic]);
    }

    #[test]
    fn test_subdiagnostic() {
        let sub_diagnostic = SubDiagnostic::new(
            SubDiagnosticType::Fail,
            "message".to_string(),
            "location".to_string(),
        );
        assert_eq!(sub_diagnostic.diagnostic_type(), &SubDiagnosticType::Fail);
        assert_eq!(sub_diagnostic.message(), "message");
        assert_eq!(sub_diagnostic.location(), "location");
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
