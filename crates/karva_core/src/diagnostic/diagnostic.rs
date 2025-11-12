use karva_project::path::TestPathError;
use pyo3::prelude::*;

use crate::{
    collection::TestCase,
    diagnostic::{
        render::{DisplayDiagnostic, DisplayDiscoveryDiagnostic},
        traceback::Traceback,
    },
    discovery::DiscoveredModule,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Diagnostic {
    TestFailure(TestFailureDiagnostic),

    Warning(WarningDiagnostic),
}

impl Diagnostic {
    pub(crate) const fn display(&self) -> DisplayDiagnostic<'_> {
        DisplayDiagnostic::new(self)
    }

    pub(crate) const fn is_test_failure(&self) -> bool {
        matches!(self, Self::TestFailure(_))
    }

    pub(crate) const fn is_warning(&self) -> bool {
        matches!(self, Self::Warning(_))
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
        Self::TestFailure(TestFailureDiagnostic::RunFailure(
            TestRunFailureDiagnostic {
                location: FunctionDefinitionLocation::new(
                    test_case.function().function_name().to_string(),
                    test_case.function().display_with_line(module),
                ),
                traceback: Traceback::new(py, error),
                message,
            },
        ))
    }

    pub(crate) fn warning(message: &str) -> Self {
        Self::Warning(WarningDiagnostic {
            message: message.to_string(),
        })
    }

    pub(crate) const fn missing_fixtures(
        missing_fixtures: Vec<String>,
        location: String,
        function_name: String,
    ) -> Self {
        Self::TestFailure(TestFailureDiagnostic::MissingFixtures(
            MissingFixturesDiagnostic {
                location: FunctionDefinitionLocation::new(location, function_name),
                missing_fixtures,
            },
        ))
    }

    pub(crate) fn into_missing_fixtures(self) -> Option<MissingFixturesDiagnostic> {
        match self {
            Self::TestFailure(TestFailureDiagnostic::MissingFixtures(diagnostic)) => {
                Some(diagnostic)
            }
            _ => None,
        }
    }

    pub(crate) const fn expect_test_failure(&self) -> &TestFailureDiagnostic {
        match self {
            Self::TestFailure(diagnostic) => diagnostic,
            Self::Warning(_) => panic!("Expected test failure"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryDiagnostic {
    InvalidFixture(InvalidFixtureDiagnostic),

    InvalidPath(TestPathError),
}

impl DiscoveryDiagnostic {
    pub(crate) const fn display(&self) -> DisplayDiscoveryDiagnostic<'_> {
        DisplayDiscoveryDiagnostic::new(self)
    }

    pub(crate) fn invalid_path_error(error: &TestPathError) -> Self {
        Self::InvalidPath(error.clone())
    }

    pub(crate) const fn invalid_fixture(
        message: String,
        location: String,
        function_name: String,
    ) -> Self {
        Self::InvalidFixture(InvalidFixtureDiagnostic {
            message,
            location: FunctionDefinitionLocation::new(location, function_name),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestFailureDiagnostic {
    RunFailure(TestRunFailureDiagnostic),
    MissingFixtures(MissingFixturesDiagnostic),
}

impl TestFailureDiagnostic {
    pub(crate) const fn location(&self) -> &FunctionDefinitionLocation {
        match self {
            Self::RunFailure(diagnostic) => &diagnostic.location,
            Self::MissingFixtures(diagnostic) => &diagnostic.location,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestRunFailureDiagnostic {
    /// The location of the test function.
    ///
    /// This is a string of the format `file.py:line`.
    pub(crate) location: FunctionDefinitionLocation,

    /// The traceback of the exception raised from the test.
    ///
    /// This comes straight from the `PyErr`.
    /// This is used as a more "debug" representation of the fail,
    /// and is not always shown.
    pub(crate) traceback: Traceback,

    /// The message of the test failure.
    ///
    /// This is used as the "info" message in the diagnostic.
    pub(crate) message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingFixturesDiagnostic {
    /// The location of the test function.
    ///
    /// This is a string of the format `file.py:line`.
    pub(crate) location: FunctionDefinitionLocation,

    /// The missing fixture names from the functions definition.
    pub(crate) missing_fixtures: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidFixtureDiagnostic {
    /// The location of the fixture function.
    ///
    /// This is a string of the format `file.py:line`.
    pub(crate) location: FunctionDefinitionLocation,

    /// The message of the fixture failure.
    ///
    /// This is used as the "info" message in the diagnostic.
    pub(crate) message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WarningDiagnostic {
    /// The message of the warning.
    ///
    /// This is used as the "info" message in the diagnostic.
    pub(crate) message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FunctionDefinitionLocation {
    pub(crate) function_name: String,

    pub(crate) location: String,
}

impl FunctionDefinitionLocation {
    const fn new(function_name: String, location: String) -> Self {
        Self {
            function_name,
            location,
        }
    }
}
