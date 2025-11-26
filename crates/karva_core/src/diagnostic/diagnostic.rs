use camino::Utf8PathBuf;
use karva_project::path::TestPathError;
use pyo3::prelude::*;

use crate::diagnostic::{
    render::{DisplayDiagnostic, DisplayDiscoveryDiagnostic, DisplayOptions},
    traceback::Traceback,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Diagnostic {
    TestFailure(TestFailureDiagnostic),

    FixtureFailure(FixtureFailureDiagnostic),

    MissingFixtures(MissingFixturesDiagnostic),

    Warning(WarningDiagnostic),
}

impl Diagnostic {
    pub(crate) const fn display_with(&self, options: DisplayOptions) -> DisplayDiagnostic<'_> {
        DisplayDiagnostic::new(self, options)
    }

    pub(crate) const fn is_test_failure(&self) -> bool {
        matches!(
            self,
            Self::TestFailure(_)
                | Self::MissingFixtures(MissingFixturesDiagnostic {
                    function_kind: FunctionKind::Test,
                    ..
                })
        )
    }

    pub(crate) const fn is_fixture_failure(&self) -> bool {
        matches!(
            self,
            Self::FixtureFailure(_)
                | Self::MissingFixtures(MissingFixturesDiagnostic {
                    function_kind: FunctionKind::Fixture,
                    ..
                })
        )
    }

    pub(crate) const fn is_warning(&self) -> bool {
        matches!(self, Self::Warning(_))
    }

    pub(crate) fn from_test_fail(
        py: Python<'_>,
        cwd: &Utf8PathBuf,
        error: &PyErr,
        location: FunctionDefinitionLocation,
    ) -> Self {
        let message = {
            let msg = error.value(py).to_string();
            if msg.is_empty() { None } else { Some(msg) }
        };
        Self::TestFailure(TestFailureDiagnostic::RunFailure(
            TestRunFailureDiagnostic {
                location,
                traceback: Traceback::new(py, cwd, error),
                message,
            },
        ))
    }

    pub(crate) const fn pass_on_expect_fail(
        reason: Option<String>,
        location: FunctionDefinitionLocation,
    ) -> Self {
        Self::TestFailure(TestFailureDiagnostic::PassOnExpectFailure(
            PassOnExpectFailureDiagnostic { location, reason },
        ))
    }

    pub(crate) fn from_fixture_fail(
        py: Python<'_>,
        cwd: &Utf8PathBuf,
        error: &PyErr,
        location: FunctionDefinitionLocation,
    ) -> Self {
        let message = {
            let msg = error.value(py).to_string();
            if msg.is_empty() { None } else { Some(msg) }
        };
        Self::FixtureFailure(FixtureFailureDiagnostic {
            location,
            traceback: Traceback::new(py, cwd, error),
            message,
        })
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
        function_kind: FunctionKind,
    ) -> Self {
        Self::MissingFixtures(MissingFixturesDiagnostic {
            location: FunctionDefinitionLocation::new(function_name, location),
            missing_fixtures,
            function_kind,
        })
    }

    pub(crate) const fn location(&self) -> Option<&FunctionDefinitionLocation> {
        match self {
            Self::TestFailure(diagnostic) => Some(diagnostic.location()),
            Self::MissingFixtures(diagnostic) => Some(&diagnostic.location),
            Self::FixtureFailure(diagnostic) => Some(&diagnostic.location),
            Self::Warning(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryDiagnostic {
    InvalidFixture(InvalidFixtureDiagnostic),

    InvalidPath(TestPathError),

    FailedToImport { path: String, error: String },
}

impl DiscoveryDiagnostic {
    pub(crate) const fn display(&self) -> DisplayDiscoveryDiagnostic<'_> {
        DisplayDiscoveryDiagnostic::new(self)
    }

    pub(crate) fn invalid_path_error(error: &TestPathError) -> Self {
        Self::InvalidPath(error.clone())
    }

    pub(crate) fn failed_to_import(path: &str, error: &str) -> Self {
        Self::FailedToImport {
            path: path.to_string(),
            error: error.to_string(),
        }
    }

    pub(crate) const fn invalid_fixture(
        message: String,
        location: String,
        function_name: String,
    ) -> Self {
        Self::InvalidFixture(InvalidFixtureDiagnostic {
            message,
            location: FunctionDefinitionLocation::new(function_name, location),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestFailureDiagnostic {
    /// The test failed on execution.
    RunFailure(TestRunFailureDiagnostic),

    PassOnExpectFailure(PassOnExpectFailureDiagnostic),
}

impl TestFailureDiagnostic {
    pub(crate) const fn location(&self) -> &FunctionDefinitionLocation {
        match self {
            Self::RunFailure(diagnostic) => &diagnostic.location,
            Self::PassOnExpectFailure(pass_on_expect_failure_diagnostic) => {
                &pass_on_expect_failure_diagnostic.location
            }
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
pub struct PassOnExpectFailureDiagnostic {
    /// The location of the test function.
    ///
    /// This is a string of the format `file.py:line`.
    pub(crate) location: FunctionDefinitionLocation,

    /// The reason that this test should fail.
    pub(crate) reason: Option<String>,
}

/// Custom diagnostic for calling a test function or fixture with missing arguments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissingFixturesDiagnostic {
    /// The location of the test function.
    ///
    /// This is a string of the format `file.py:line`.
    pub(crate) location: FunctionDefinitionLocation,

    /// The missing fixture names from the functions definition.
    pub(crate) missing_fixtures: Vec<String>,

    /// The kind of function that is missing fixtures.
    pub(crate) function_kind: FunctionKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixtureFailureDiagnostic {
    /// The location of the fixture function.
    ///
    /// This is a string of the format `file.py:line`.
    pub(crate) location: FunctionDefinitionLocation,

    /// The traceback of the fixture failure.
    ///
    /// This is used as the "info" message in the diagnostic.
    pub(crate) traceback: Traceback,

    /// The message of the fixture failure.
    ///
    /// This is used as the "info" message in the diagnostic.
    pub(crate) message: Option<String>,
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
pub struct FunctionDefinitionLocation {
    pub(crate) function_name: String,

    pub(crate) location: String,
}

impl FunctionDefinitionLocation {
    pub(crate) const fn new(function_name: String, location: String) -> Self {
        Self {
            function_name,
            location,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FunctionKind {
    Fixture,
    Test,
}
