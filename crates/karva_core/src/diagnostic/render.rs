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

        writeln!(
            f,
            " --> {}{}",
            function_name.map_or_else(String::new, |function_name| format!(
                "{}",
                function_name.bold()
            )),
            self.diagnostic
                .location()
                .map_or_else(String::new, |location| format!(" at {location}"))
        )?;

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
                    FixtureSubDiagnosticType::NotFound => "error[fixture-not-found]".yellow(),
                },
                SubDiagnosticErrorType::Unknown => "error".yellow(),
            },
            SubDiagnosticSeverity::Warning(error) => {
                format!("warning[{}]", to_kebab_case(error)).yellow()
            }
        };

        writeln!(
            f,
            "{diagnostic_type_label}{}",
            self.diagnostic
                .location()
                .map_or_else(String::new, |location| format!(" in {location}"))
        )?;

        Ok(())
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_diagnostic_display() {
//         let mut diagnostic = Diagnostic::new(
//             "message".to_string(),
//             None,
//             DiagnosticSeverity::Error(DiagnosticErrorType::TestCase(
//                 "test_fail".to_string(),
//                 TestCaseDiagnosticType::Fail,
//             )),
//         );
//         diagnostic.add_sub_diagnostics(vec![
//             SubDiagnostic::new(
//                 "This test should fail".to_string(),
//                 Some("test_fail.py:4".to_string()),
//                 SubDiagnosticSeverity::Error(SubDiagnosticErrorType::Unknown),
//             ),
//             SubDiagnostic::new(
//                 "This is an error".to_string(),
//                 Some("test_error.py:8".to_string()),
//                 SubDiagnosticSeverity::Error(SubDiagnosticErrorType::Unknown),
//             ),
//             SubDiagnostic::new(
//                 "This is an error".to_string(),
//                 Some("test_error.py:8".to_string()),
//                 SubDiagnosticSeverity::Error(SubDiagnosticErrorType::Unknown),
//             ),
//         ]);

//         let display = diagnostic.display();
//         let expected = format!(
//             "{} in test_fail.py:4\n | This test should fail\n{} in test_error.py:8\n | This is an error\n",
//             "fail[assertion-failed]".red(),
//             "error[value-error]".yellow()
//         );

//         assert_eq!(display.to_string(), expected);
//     }

//     #[test]
//     fn test_sub_diagnostic_fail_display() {
//         let diagnostic = DiagnosticInner::new(
//             "test_fixture_function_name".to_string(),
//             Some("test_fixture_function_name.py".to_string()),
//             DiagnosticSeverity::Error(DiagnosticErrorType::TestCase(
//                 "test_fixture_function_name".to_string(),
//                 TestCaseDiagnosticType::Fail,
//             )),
//         );
//         let display = DiagnosticInnerDisplay::new(&diagnostic);
//         assert_eq!(
//             display.to_string(),
//             "fail[assertion-failed]".red().to_string()
//                 + " in test_fixture_function_name.py\n | test_fixture_function_name\n"
//         );
//     }

//     #[test]
//     fn test_sub_diagnostic_error_display() {
//         let diagnostic = DiagnosticInner::new(
//             "test_fixture_function_name".to_string(),
//             Some("test_fixture_function_name.py".to_string()),
//             DiagnosticSeverity::Error(DiagnosticErrorType::TestCase(
//                 "test_fixture_function_name".to_string(),
//                 TestCaseDiagnosticType::Error("ValueError".to_string()),
//             )),
//         );
//         let display = DiagnosticInnerDisplay::new(&diagnostic);
//         assert_eq!(
//             display.to_string(),
//             "error[value-error]".yellow().to_string()
//                 + " in test_fixture_function_name.py\n | test_fixture_function_name\n"
//         );
//     }

//     #[test]
//     fn test_sub_diagnostic_fixture_not_found_display() {
//         let diagnostic = SubDiagnostic::fixture_not_found(
//             &"fixture_name".to_string(),
//             Some("test_fixture_function_name.py".to_string()),
//         );
//         assert_eq!(
//             diagnostic.display().to_string(),
//             "error[fixture-not-found]".yellow().to_string()
//                 + " in test_fixture_function_name.py\n | Fixture fixture_name not found\n"
//         );
//     }

//     #[test]
//     fn test_sub_diagnostic_invalid_fixture_display() {
//         let diagnostic = Diagnostic::invalid_fixture(
//             "fixture_name".to_string(),
//             Some("test_fixture_function_name.py".to_string()),
//         );
//         assert_eq!(
//             diagnostic.display().to_string(),
//             "error[invalid-fixture]".yellow().to_string()
//                 + " in test_fixture_function_name.py\n | fixture_name\n"
//         );
//     }
// }
