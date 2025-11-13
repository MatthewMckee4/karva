use std::fmt::{self, Display, Formatter};

use crate::diagnostic::{
    Diagnostic, DiscoveryDiagnostic, FunctionKind, InvalidFixtureDiagnostic,
    MissingFixturesDiagnostic, TestFailureDiagnostic, WarningDiagnostic,
    diagnostic::{FixtureFailureDiagnostic, FunctionDefinitionLocation},
};

pub struct DisplayDiagnostic<'a> {
    diagnostic: &'a Diagnostic,
}

impl<'a> DisplayDiagnostic<'a> {
    pub(crate) const fn new(diagnostic: &'a Diagnostic) -> Self {
        Self { diagnostic }
    }
}

impl Display for DisplayDiagnostic<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.diagnostic {
            Diagnostic::TestFailure(TestFailureDiagnostic {
                location:
                    FunctionDefinitionLocation {
                        function_name,
                        location: function_location,
                    },
                traceback,
                message,
            }) => {
                let location_fail_string = traceback
                    .location
                    .as_ref()
                    .map(|location| format!("at {location}"))
                    .unwrap_or_default();

                writeln!(
                    f,
                    "test `{function_name}` at {function_location} failed {location_fail_string}"
                )?;
                if let Some(message) = message {
                    writeln!(f, "{message}")?;
                }
            }
            Diagnostic::MissingFixtures(MissingFixturesDiagnostic {
                location:
                    FunctionDefinitionLocation {
                        function_name,
                        location,
                    },
                missing_fixtures,
                function_kind,
            }) => {
                writeln!(
                    f,
                    "{function_kind} `{function_name}` has missing fixtures: {missing_fixtures:?} at {location}",
                )?;
            }
            Diagnostic::Warning(WarningDiagnostic { message }) => {
                writeln!(f, "warning: {message}")?;
            }
            Diagnostic::FixtureFailure(FixtureFailureDiagnostic {
                location:
                    FunctionDefinitionLocation {
                        function_name,
                        location: function_location,
                    },
                traceback,
                message,
            }) => {
                let location_fail_string = traceback
                    .location
                    .as_ref()
                    .map(|location| format!("at {location}"))
                    .unwrap_or_default();

                writeln!(
                    f,
                    "fixture function `{function_name}` at {function_location} failed {location_fail_string}"
                )?;
                if let Some(message) = message {
                    writeln!(f, "{message}")?;
                }
            }
        }

        Ok(())
    }
}

pub struct DisplayDiscoveryDiagnostic<'a> {
    diagnostic: &'a DiscoveryDiagnostic,
}

impl<'a> DisplayDiscoveryDiagnostic<'a> {
    pub(crate) const fn new(diagnostic: &'a DiscoveryDiagnostic) -> Self {
        Self { diagnostic }
    }
}

impl Display for DisplayDiscoveryDiagnostic<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.diagnostic {
            DiscoveryDiagnostic::InvalidFixture(InvalidFixtureDiagnostic {
                location:
                    FunctionDefinitionLocation {
                        function_name,
                        location,
                    },
                message,
            }) => {
                writeln!(
                    f,
                    "invalid fixture `{function_name}`: {message} at {location}"
                )?;
            }
            DiscoveryDiagnostic::InvalidPath(test_path_error) => {
                writeln!(f, "{test_path_error}")?;
            }
        }

        Ok(())
    }
}

impl Display for FunctionKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Test => write!(f, "test"),
            Self::Fixture => write!(f, "fixture"),
        }
    }
}
