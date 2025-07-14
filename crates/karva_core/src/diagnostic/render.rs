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
    #[must_use]
    pub const fn new(diagnostic: &'a Diagnostic) -> Self {
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
    #[must_use]
    pub const fn new(diagnostic: &'a DiagnosticInner) -> Self {
        Self { diagnostic }
    }
}

impl std::fmt::Display for DiagnosticInnerDisplay<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let diagnostic_type_label = match self.diagnostic.severity() {
            DiagnosticSeverity::Error(error_type) => match error_type {
                DiagnosticErrorType::TestCase(_, test_case_type) => match test_case_type {
                    TestCaseDiagnosticType::Fail => "fail[assertion-failed]".red(),
                    TestCaseDiagnosticType::Error(error) => {
                        format!("error[{}]", to_kebab_case(error)).yellow()
                    }
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
                DiagnosticErrorType::Unknown => "error".yellow(),
                DiagnosticErrorType::Fixture(fixture_type) => match fixture_type {
                    FixtureDiagnosticType::Invalid => "error[invalid-fixture]".yellow(),
                },
            },
            DiagnosticSeverity::Warning(error) => {
                format!("warning[{}]", to_kebab_case(error)).yellow()
            }
        };

        let function_name = match self.diagnostic.severity() {
            DiagnosticSeverity::Error(DiagnosticErrorType::TestCase(function_name, _)) => {
                Some(function_name)
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

pub struct SubDiagnosticDisplay<'a> {
    diagnostic: &'a SubDiagnostic,
}

impl<'a> SubDiagnosticDisplay<'a> {
    #[must_use]
    pub const fn new(diagnostic: &'a SubDiagnostic) -> Self {
        Self { diagnostic }
    }
}

impl std::fmt::Display for SubDiagnosticDisplay<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let diagnostic_type_label = match self.diagnostic.severity() {
            SubDiagnosticSeverity::Error(error_type) => match error_type {
                SubDiagnosticErrorType::Fixture(fixture_type) => match fixture_type {
                    FixtureSubDiagnosticType::NotFound(_) => "error (fixture-not-found)".yellow(),
                },
                SubDiagnosticErrorType::Unknown => "error".yellow(),
            },
            SubDiagnosticSeverity::Warning(error) => {
                format!("warning ({})", to_kebab_case(error)).yellow()
            }
        };

        writeln!(f, "{diagnostic_type_label}: {}", self.diagnostic.message())?;

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use crate::diagnostic::{
        Diagnostic, DiagnosticErrorType, DiagnosticInner, DiagnosticSeverity,
        FixtureSubDiagnosticType, SubDiagnostic, SubDiagnosticErrorType, SubDiagnosticSeverity,
        TestCaseCollectionDiagnosticType, TestCaseDiagnosticType,
        diagnostic::FixtureDiagnosticType,
    };

    fn strip_ansi_codes(input: &str) -> String {
        let re = regex::Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap();
        re.replace_all(input, "").to_string()
    }

    mod diagnostic_inner_display_tests {
        use super::*;

        #[test]
        fn test_test_case_fail() {
            let diagnostic = DiagnosticInner::new(
                Some("Test assertion failed".to_string()),
                Some("test_example.py:10".to_string()),
                Some("Traceback info".to_string()),
                DiagnosticSeverity::Error(DiagnosticErrorType::TestCase(
                    "test_example".to_string(),
                    TestCaseDiagnosticType::Fail,
                )),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "fail[assertion-failed]: Test assertion failed\n --> test_example at test_example.py:10\nTraceback info\n";
            assert_eq!(output, expected);
        }

        #[test]
        fn test_test_case_error() {
            let diagnostic = DiagnosticInner::new(
                Some("RuntimeError occurred".to_string()),
                Some("test_example.py:15".to_string()),
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::TestCase(
                    "test_runtime_error".to_string(),
                    TestCaseDiagnosticType::Error("RuntimeError".to_string()),
                )),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "error[runtime-error]: RuntimeError occurred\n --> test_runtime_error at test_example.py:15\n";
            assert_eq!(output, expected);
        }

        #[test]
        fn test_test_case_collection_fixture_not_found() {
            let diagnostic = DiagnosticInner::new(
                Some("Fixture not found".to_string()),
                Some("test_example.py:20".to_string()),
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::TestCase(
                    "test_with_fixture".to_string(),
                    TestCaseDiagnosticType::Collection(
                        TestCaseCollectionDiagnosticType::FixtureNotFound,
                    ),
                )),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "error[fixtures-not-found]: Fixture not found\n --> test_with_fixture at test_example.py:20\n";
            assert_eq!(output, expected);
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

            let expected = "error[invalid-path]: Known error occurred\n --> file.py:5\n";
            assert_eq!(output, expected);
        }

        #[test]
        fn test_unknown_error() {
            let diagnostic = DiagnosticInner::new(
                Some("Unknown error".to_string()),
                Some("unknown.py:1".to_string()),
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::Unknown),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "error: Unknown error\n --> unknown.py:1\n";
            assert_eq!(output, expected);
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

            let expected =
                "error[invalid-fixture]: Invalid fixture definition\n --> conftest.py:10\n";
            assert_eq!(output, expected);
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

            let expected = "warning[deprecation-warning]: This is a warning\n --> warning.py:5\n";
            assert_eq!(output, expected);
        }

        #[test]
        fn test_no_message() {
            let diagnostic = DiagnosticInner::new(
                None,
                Some("test.py:1".to_string()),
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::Unknown),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "error\n --> test.py:1\n";
            assert_eq!(output, expected);
        }

        #[test]
        fn test_no_location() {
            let diagnostic = DiagnosticInner::new(
                Some("Error with no location".to_string()),
                None,
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::Unknown),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "error: Error with no location\n";
            assert_eq!(output, expected);
        }

        #[test]
        fn test_no_message_no_location() {
            let diagnostic = DiagnosticInner::new(
                None,
                None,
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::Unknown),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "error\n";
            assert_eq!(output, expected);
        }

        #[test]
        fn test_kebab_case_conversion_in_error() {
            let diagnostic = DiagnosticInner::new(
                Some("Error message".to_string()),
                None,
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::TestCase(
                    "test_func".to_string(),
                    TestCaseDiagnosticType::Error("ValueError".to_string()),
                )),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "error[value-error]: Error message\n";
            assert_eq!(output, expected);
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

            let expected = "warning[deprecation-warning]: Warning message\n";
            assert_eq!(output, expected);
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

            let expected = "error (fixture-not-found): fixture 'my_fixture' not found\n";
            assert_eq!(output, expected);
        }

        #[test]
        fn test_unknown_error() {
            let sub_diagnostic = SubDiagnostic::new(
                "Unknown sub-diagnostic error".to_string(),
                SubDiagnosticSeverity::Error(SubDiagnosticErrorType::Unknown),
            );

            let display = sub_diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "error: Unknown sub-diagnostic error\n";
            assert_eq!(output, expected);
        }

        #[test]
        fn test_warning() {
            let sub_diagnostic = SubDiagnostic::new(
                "This is a sub-diagnostic warning".to_string(),
                SubDiagnosticSeverity::Warning("TestWarning".to_string()),
            );

            let display = sub_diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "warning (test-warning): This is a sub-diagnostic warning\n";
            assert_eq!(output, expected);
        }

        #[test]
        fn test_kebab_case_conversion_in_sub_warning() {
            let sub_diagnostic = SubDiagnostic::new(
                "Warning message".to_string(),
                SubDiagnosticSeverity::Warning("SomeComplexWarning".to_string()),
            );

            let display = sub_diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "warning (some-complex-warning): Warning message\n";
            assert_eq!(output, expected);
        }
    }

    mod display_diagnostic_tests {
        use super::*;

        #[test]
        fn test_diagnostic_with_sub_diagnostics() {
            let mut diagnostic = Diagnostic::new(
                Some("Main error".to_string()),
                Some("test.py:10".to_string()),
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::TestCase(
                    "test_main".to_string(),
                    TestCaseDiagnosticType::Collection(
                        TestCaseCollectionDiagnosticType::FixtureNotFound,
                    ),
                )),
            );

            let sub_diagnostic1 = SubDiagnostic::new(
                "fixture 'fixture1' not found".to_string(),
                SubDiagnosticSeverity::Error(SubDiagnosticErrorType::Fixture(
                    FixtureSubDiagnosticType::NotFound("fixture1".to_string()),
                )),
            );

            let sub_diagnostic2 = SubDiagnostic::new(
                "fixture 'fixture2' not found".to_string(),
                SubDiagnosticSeverity::Error(SubDiagnosticErrorType::Fixture(
                    FixtureSubDiagnosticType::NotFound("fixture1".to_string()),
                )),
            );

            diagnostic.add_sub_diagnostic(sub_diagnostic1);
            diagnostic.add_sub_diagnostic(sub_diagnostic2);

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "error[fixtures-not-found]: Main error\n --> test_main at test.py:10\nerror (fixture-not-found): fixture 'fixture1' not found\nerror (fixture-not-found): fixture 'fixture2' not found\n";
            assert_eq!(output, expected);
        }

        #[test]
        fn test_diagnostic_without_sub_diagnostics() {
            let diagnostic = Diagnostic::new(
                Some("Simple error".to_string()),
                Some("simple.py:5".to_string()),
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::Unknown),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "error: Simple error\n --> simple.py:5\n";
            assert_eq!(output, expected);
        }

        #[test]
        fn test_diagnostic_with_mixed_sub_diagnostics() {
            let mut diagnostic = Diagnostic::new(
                Some("Mixed error".to_string()),
                Some("mixed.py:1".to_string()),
                None,
                DiagnosticSeverity::Warning("TestWarning".to_string()),
            );

            let sub_error = SubDiagnostic::new(
                "Sub error".to_string(),
                SubDiagnosticSeverity::Error(SubDiagnosticErrorType::Unknown),
            );

            let sub_warning = SubDiagnostic::new(
                "Sub warning".to_string(),
                SubDiagnosticSeverity::Warning("SubWarning".to_string()),
            );

            diagnostic.add_sub_diagnostic(sub_error);
            diagnostic.add_sub_diagnostic(sub_warning);

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "warning[test-warning]: Mixed error\n --> mixed.py:1\nerror: Sub error\nwarning (sub-warning): Sub warning\n";
            assert_eq!(output, expected);
        }

        #[test]
        fn test_unknown_error() {
            let diagnostic = Diagnostic::unknown_error(
                Some("Unknown error".to_string()),
                Some("unknown.py:1".to_string()),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "error: Unknown error\n --> unknown.py:1\n";
            assert_eq!(output, expected);
        }
    }

    mod edge_case_tests {
        use super::*;

        #[test]
        fn test_empty_strings() {
            let diagnostic = DiagnosticInner::new(
                Some(String::new()),
                Some(String::new()),
                Some(String::new()),
                DiagnosticSeverity::Error(DiagnosticErrorType::Unknown),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "error: \n --> \n\n";
            assert_eq!(output, expected);
        }

        #[test]
        fn test_long_strings() {
            let long_message = "a".repeat(1000);
            let long_location = "b".repeat(500);
            let long_traceback = "c".repeat(2000);

            let diagnostic = DiagnosticInner::new(
                Some(long_message.clone()),
                Some(long_location.clone()),
                Some(long_traceback.clone()),
                DiagnosticSeverity::Error(DiagnosticErrorType::Unknown),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            assert!(output.contains(&long_message));
            assert!(output.contains(&long_location));
            assert!(output.contains(&long_traceback));
        }

        #[test]
        fn test_special_characters() {
            let diagnostic = DiagnosticInner::new(
                Some("Error with special chars: \n\t\"'\\".to_string()),
                Some("file with spaces.py:10".to_string()),
                None,
                DiagnosticSeverity::Error(DiagnosticErrorType::Known("SpecialError".to_string())),
            );

            let display = diagnostic.display();
            let output = strip_ansi_codes(&display.to_string());

            let expected = "error[special-error]: Error with special chars: \n\t\"'\\\n --> file with spaces.py:10\n";
            assert_eq!(output, expected);
        }
    }
}
