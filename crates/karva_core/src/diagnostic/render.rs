use crate::diagnostic::{
    Diagnostic, DiscoveryDiagnostic, InvalidFixtureDiagnostic, MissingFixturesDiagnostic,
    TestFailureDiagnostic, WarningDiagnostic,
    diagnostic::{FunctionDefinitionLocation, TestRunFailureDiagnostic},
};

pub struct DisplayDiagnostic<'a> {
    diagnostic: &'a Diagnostic,
}

impl<'a> DisplayDiagnostic<'a> {
    pub(crate) const fn new(diagnostic: &'a Diagnostic) -> Self {
        Self { diagnostic }
    }
}

impl std::fmt::Display for DisplayDiagnostic<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.diagnostic {
            Diagnostic::TestFailure(test_failure_diagnostic) => match test_failure_diagnostic {
                TestFailureDiagnostic::RunFailure(TestRunFailureDiagnostic {
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
                TestFailureDiagnostic::MissingFixtures(MissingFixturesDiagnostic {
                    location:
                        FunctionDefinitionLocation {
                            function_name,
                            location,
                        },
                    missing_fixtures,
                }) => {
                    writeln!(
                        f,
                        "test `{function_name}` has missing fixtures: {missing_fixtures:?} at {location}",
                    )?;
                }
            },

            Diagnostic::Warning(WarningDiagnostic { message }) => {
                writeln!(f, "warning: {message}")?;
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

impl std::fmt::Display for DisplayDiscoveryDiagnostic<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
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
