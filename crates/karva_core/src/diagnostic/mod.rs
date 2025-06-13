use pyo3::{prelude::*, types::PyString};

use crate::{
    case::TestCase,
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

    pub fn from_test_fail(py: Python<'_>, error: &PyErr, test_case: &TestCase) -> Self {
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
    pub fn fixture_not_found(fixture_name: &str, test_case: &TestCase) -> Self {
        Self::new(
            vec![SubDiagnostic {
                diagnostic_type: SubDiagnosticType::Error(DiagnosticError::FixtureNotFound(
                    fixture_name.to_string(),
                )),
                message: format!("Fixture {fixture_name} not found"),
                location: test_case.path().to_string(),
            }],
            DiagnosticScope::Setup,
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

#[derive(Clone, Debug)]
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
        .unwrap_or_else(|_| PyString::new(py, "Unknown"))
        .to_string()
}

// Simplified traceback filtering that removes unnecessary traceback headers
fn filter_traceback(traceback: &str) -> String {
    let lines: Vec<&str> = traceback.lines().collect();
    let mut filtered = String::new();

    for (i, line) in lines.iter().enumerate() {
        if i == 0 && line.contains("Traceback (most recent call last):") {
            continue;
        }
        if line.starts_with("  ") {
            if let Some(stripped) = line.strip_prefix("  ") {
                filtered.push_str(stripped);
            }
        } else {
            filtered.push_str(line);
        }
        filtered.push('\n');
    }
    filtered = filtered.trim_end_matches('\n').to_string();

    filtered = filtered.trim_end_matches('^').to_string();

    filtered.trim_end().to_string()
}
