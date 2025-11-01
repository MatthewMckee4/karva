use colored::Colorize;

use crate::diagnostic::{
    Diagnostic, DiagnosticErrorType, DiagnosticInner, DiagnosticSeverity, FixtureSubDiagnosticType,
    SubDiagnostic, SubDiagnosticErrorType, SubDiagnosticSeverity, TestCaseCollectionDiagnosticType,
    TestCaseDiagnosticType, diagnostic::FixtureDiagnosticType, utils::to_kebab_case,
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
        write!(f, "{}", self.diagnostic.inner().display())?;

        for sub_diagnostic in self.diagnostic.sub_diagnostics() {
            write!(f, "{}", sub_diagnostic.display())?;
        }

        Ok(())
    }
}

pub struct DiagnosticInnerDisplay<'a> {
    diagnostic: &'a DiagnosticInner,
}

impl<'a> DiagnosticInnerDisplay<'a> {
    pub(crate) const fn new(diagnostic: &'a DiagnosticInner) -> Self {
        Self { diagnostic }
    }
}

impl std::fmt::Display for DiagnosticInnerDisplay<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let diagnostic_type_label = match self.diagnostic.severity() {
            DiagnosticSeverity::Error(error_type) => match error_type {
                DiagnosticErrorType::TestCase {
                    diagnostic_type, ..
                } => match diagnostic_type {
                    TestCaseDiagnosticType::Fail(error) => format!("fail[{error}]").red(),
                    TestCaseDiagnosticType::Collection(test_case_collection_type) => {
                        match test_case_collection_type {
                            TestCaseCollectionDiagnosticType::FixtureNotFound => {
                                "error[fixtures-not-found]".yellow()
                            }
                        }
                    }
                },
                DiagnosticErrorType::Known(error) => {
                    format!("error[{}]", to_kebab_case(error)).yellow()
                }
                DiagnosticErrorType::Fixture(fixture_type) => match fixture_type {
                    FixtureDiagnosticType::Invalid => "error[invalid-fixture]".yellow(),
                },
            },
            DiagnosticSeverity::Warning(error) => {
                format!("warning[{}]", to_kebab_case(error)).yellow()
            }
        };

        let function_name = match self.diagnostic.severity() {
            DiagnosticSeverity::Error(DiagnosticErrorType::TestCase { test_name, .. }) => {
                Some(test_name)
            }
            _ => None,
        };

        writeln!(
            f,
            "{diagnostic_type_label}{}",
            self.diagnostic
                .message()
                .map_or_else(String::new, |message| format!(": {message}"))
        )?;

        if let Some(location) = self.diagnostic.location() {
            if let Some(function_name) = function_name {
                writeln!(f, " --> {function_name} at {location}")?;
            } else {
                writeln!(f, " --> {location}")?;
            }
        }

        if let Some(traceback) = self.diagnostic.traceback() {
            writeln!(f, "{traceback}")?;
        }

        Ok(())
    }
}

pub(crate) struct SubDiagnosticDisplay<'a> {
    diagnostic: &'a SubDiagnostic,
}

impl<'a> SubDiagnosticDisplay<'a> {
    pub(crate) const fn new(diagnostic: &'a SubDiagnostic) -> Self {
        Self { diagnostic }
    }
}

impl std::fmt::Display for SubDiagnosticDisplay<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let diagnostic_type_label = match self.diagnostic.severity() {
            SubDiagnosticSeverity::Error(error_type) => match error_type {
                SubDiagnosticErrorType::Fixture(fixture_type) => match fixture_type {
                    FixtureSubDiagnosticType::NotFound(_) => "error[fixture-not-found]".yellow(),
                },
            },
        };

        writeln!(f, "{diagnostic_type_label}: {}", self.diagnostic.message())?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::diagnostic::{
        DiagnosticErrorType, DiagnosticInner, DiagnosticSeverity, FixtureSubDiagnosticType,
        SubDiagnostic, SubDiagnosticErrorType, SubDiagnosticSeverity,
        TestCaseCollectionDiagnosticType, TestCaseDiagnosticType,
        diagnostic::FixtureDiagnosticType,
    };

    fn strip_ansi_codes(input: &str) -> String {
        let re = regex::Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap();
        re.replace_all(input, "").to_string()
    }

    mod diagnostic_inner_display_tests {
        use super::*;
        use crate::diagnostic::utils::to_kebab_case;

        #[test]
        fn test_test_case_fail() {
            let diagnostic = DiagnosticInner::new(
                Some("Test assertion failed".to_string()),
                Some("test_example.py:10".to_string()),
                Some("Traceback info".to_string()),
                DiagnosticSeverity::Error(DiagnosticErrorType::TestCase {
                    test_name: "test_example".to_string(),
                    diagnostic_type: TestCaseDiagnosticType::Fail(to_kebab_case("SomeError")),
                }),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            insta::assert_snapshot!(output, @r"
            fail[some-error]: Test assertion failed
             --> test_example at test_example.py:10
            Traceback info
            ");
        }

        #[test]
        fn test_test_case_collection_fixture_not_found() {
            let diagnostic = DiagnosticInner::new(
                Some("Fixture not found".to_string()),
                Some("test_example.py:20".to_string()),
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::TestCase {
                    test_name: "test_with_fixture".to_string(),
                    diagnostic_type: TestCaseDiagnosticType::Collection(
                        TestCaseCollectionDiagnosticType::FixtureNotFound,
                    ),
                }),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            insta::assert_snapshot!(output, @r###"
            error[fixtures-not-found]: Fixture not found
             --> test_with_fixture at test_example.py:20
            "###);
        }

        #[test]
        fn test_known_error() {
            let diagnostic = DiagnosticInner::new(
                Some("Known error occurred".to_string()),
                Some("file.py:5".to_string()),
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::Known("InvalidPath".to_string())),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            insta::assert_snapshot!(output, @r###"
            error[invalid-path]: Known error occurred
             --> file.py:5
            "###);
        }

        #[test]
        fn test_unknown_error() {
            let diagnostic = DiagnosticInner::new(
                Some("Unknown error".to_string()),
                Some("unknown.py:1".to_string()),
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::Known("UnknownError".to_string())),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            insta::assert_snapshot!(output, @r###"
            error[unknown-error]: Unknown error
             --> unknown.py:1
            "###);
        }

        #[test]
        fn test_fixture_invalid() {
            let diagnostic = DiagnosticInner::new(
                Some("Invalid fixture definition".to_string()),
                Some("conftest.py:10".to_string()),
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::Fixture(
                    FixtureDiagnosticType::Invalid,
                )),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            insta::assert_snapshot!(output, @r###"
            error[invalid-fixture]: Invalid fixture definition
             --> conftest.py:10
            "###);
        }

        #[test]
        fn test_warning() {
            let diagnostic = DiagnosticInner::new(
                Some("This is a warning".to_string()),
                Some("warning.py:5".to_string()),
                None,
                DiagnosticSeverity::Warning("DeprecationWarning".to_string()),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            insta::assert_snapshot!(output, @r###"
            warning[deprecation-warning]: This is a warning
             --> warning.py:5
            "###);
        }

        #[test]
        fn test_no_message() {
            let diagnostic = DiagnosticInner::new(
                None,
                Some("test.py:1".to_string()),
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::Known("UnknownError".to_string())),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            insta::assert_snapshot!(output, @r###"
            error[unknown-error]
             --> test.py:1
            "###);
        }

        #[test]
        fn test_no_location() {
            let diagnostic = DiagnosticInner::new(
                Some("Error with no location".to_string()),
                None,
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::Known("UnknownError".to_string())),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            insta::assert_snapshot!(output, @r###"
            error[unknown-error]: Error with no location
            "###);
        }

        #[test]
        fn test_no_message_no_location() {
            let diagnostic = DiagnosticInner::new(
                None,
                None,
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::Known("UnknownError".to_string())),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            insta::assert_snapshot!(output, @r###"
            error[unknown-error]
            "###);
        }

        #[test]
        fn test_kebab_case_conversion_in_warning() {
            let diagnostic = DiagnosticInner::new(
                Some("Warning message".to_string()),
                None,
                None,
                DiagnosticSeverity::Warning("DeprecationWarning".to_string()),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            insta::assert_snapshot!(output, @r###"
            warning[deprecation-warning]: Warning message
            "###);
        }
    }

    mod sub_diagnostic_display_tests {
        use super::*;

        #[test]
        fn test_fixture_not_found() {
            let sub_diagnostic = SubDiagnostic::new(
                "fixture 'my_fixture' not found".to_string(),
                SubDiagnosticSeverity::Error(SubDiagnosticErrorType::Fixture(
                    FixtureSubDiagnosticType::NotFound("fixture1".to_string()),
                )),
            );

            let display = sub_diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            insta::assert_snapshot!(output, @"error[fixture-not-found]: fixture 'my_fixture' not found");
        }
    }

    mod edge_case_tests {
        use super::*;

        #[test]
        fn test_special_characters() {
            let diagnostic = DiagnosticInner::new(
                Some("Error with special chars:\n\t\"'\\".to_string()),
                Some("file with spaces.py:10".to_string()),
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::Known("SpecialError".to_string())),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            insta::assert_snapshot!(output, @r#"
            error[special-error]: Error with special chars:
            	"'\
             --> file with spaces.py:10
            "#);
        }
    }
}
