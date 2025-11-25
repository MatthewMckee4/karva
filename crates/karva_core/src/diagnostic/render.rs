use std::fmt::{self, Display, Formatter};

use karva_project::Project;

use crate::diagnostic::{
    Diagnostic, DiscoveryDiagnostic, FunctionKind, InvalidFixtureDiagnostic,
    MissingFixturesDiagnostic, PassOnExpectFailureDiagnostic, TestFailureDiagnostic,
    TestRunFailureDiagnostic, WarningDiagnostic,
    diagnostic::{FixtureFailureDiagnostic, FunctionDefinitionLocation},
};

#[derive(Debug, Clone, Copy, Default)]
pub struct DisplayOptions {
    pub show_traceback: bool,
}

impl From<&Project> for DisplayOptions {
    fn from(project: &Project) -> Self {
        Self {
            show_traceback: project.options().show_traceback(),
        }
    }
}

pub struct DisplayDiagnostic<'a> {
    diagnostic: &'a Diagnostic,
    options: DisplayOptions,
}

impl<'a> DisplayDiagnostic<'a> {
    pub(crate) const fn new(diagnostic: &'a Diagnostic, options: DisplayOptions) -> Self {
        Self {
            diagnostic,
            options,
        }
    }
}

impl Display for DisplayDiagnostic<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.diagnostic {
            Diagnostic::TestFailure(test_failure_diagnostic) => match test_failure_diagnostic {
                TestFailureDiagnostic::RunFailure(run_failure_diagnostic) => {
                    let TestRunFailureDiagnostic {
                        location:
                            FunctionDefinitionLocation {
                                function_name,
                                location: function_location,
                            },
                        traceback,
                        message,
                    } = run_failure_diagnostic;

                    let location_fail_string = traceback
                        .location
                        .as_ref()
                        .map(|location| format!("at {location}"))
                        .unwrap_or_default();

                    writeln!(
                        f,
                        "test `{function_name}` at {function_location} failed {location_fail_string}"
                    )?;
                    if self.options.show_traceback {
                        for line in &traceback.lines {
                            writeln!(f, "{line}")?;
                        }
                    } else if let Some(message) = message {
                        writeln!(f, "{message}")?;
                    }
                }
                TestFailureDiagnostic::PassOnExpectFailure(pass_on_expect_failure_diagnostic) => {
                    let PassOnExpectFailureDiagnostic {
                        location:
                            FunctionDefinitionLocation {
                                function_name,
                                location: function_location,
                            },
                        reason,
                    } = pass_on_expect_failure_diagnostic;

                    writeln!(
                        f,
                        "test `{function_name}` at {function_location} passed when it was expected to fail"
                    )?;
                    if let Some(reason) = reason {
                        writeln!(f, "reason: {reason}")?;
                    }
                }
            },
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
            DiscoveryDiagnostic::FailedToImport { path, error } => {
                writeln!(f, "failed to import module `{path}`: {error}")?;
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
