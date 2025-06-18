use colored::Colorize;

use crate::diagnostic::{Diagnostic, DiagnosticError, SubDiagnostic, SubDiagnosticType};

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
        for sub_diagnostic in self.diagnostic.sub_diagnostics() {
            write!(f, "{}", sub_diagnostic.display())?;
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
        let diagnostic_type_str = match self.diagnostic.diagnostic_type() {
            SubDiagnosticType::Fail => "fail[assertion-failed]".red(),
            SubDiagnosticType::Error(error) => match error {
                DiagnosticError::Error(type_name) => {
                    format!("error[{}]", to_kebab_case(type_name)).yellow()
                }
                DiagnosticError::FixtureNotFound(_) => {
                    "error[fixture-not-found]".to_string().yellow()
                }
                DiagnosticError::InvalidFixture(_) => "error[invalid-fixture]".to_string().yellow(),
                DiagnosticError::InvalidPath(_) => "error[invalid-path]".to_string().yellow(),
            },
        };
        writeln!(
            f,
            "{} in {}",
            diagnostic_type_str,
            self.diagnostic.location()
        )?;

        for line in self.diagnostic.message.lines() {
            writeln!(f, " | {line}")?;
        }

        Ok(())
    }
}

fn to_kebab_case(input: &str) -> String {
    input
        .chars()
        .enumerate()
        .fold(String::new(), |mut acc, (i, c)| {
            if i > 0 && c.is_uppercase() {
                acc.push('-');
            }
            acc.push(c.to_ascii_lowercase());
            acc
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::DiagnosticScope;

    #[test]
    fn test_to_kebab_case() {
        assert_eq!(to_kebab_case("FooBar"), "foo-bar");
    }

    #[test]
    fn test_diagnostic_display() {
        let diagnostic = Diagnostic::new(
            vec![
                SubDiagnostic::new(
                    SubDiagnosticType::Fail,
                    "This test should fail".to_string(),
                    "test_fail.py:4".to_string(),
                ),
                SubDiagnostic::new(
                    SubDiagnosticType::Error(DiagnosticError::Error("ValueError".to_string())),
                    "This is an error".to_string(),
                    "test_error.py:8".to_string(),
                ),
            ],
            DiagnosticScope::Test,
        );

        let display = diagnostic.display();
        let expected = format!(
            "{} in test_fail.py:4\n | This test should fail\n{} in test_error.py:8\n | This is an error\n",
            "fail[assertion-failed]".red(),
            "error[value-error]".yellow()
        );

        assert_eq!(display.to_string(), expected);
    }

    #[test]
    fn test_sub_diagnostic_fail_display() {
        let diagnostic = SubDiagnostic::new(
            SubDiagnosticType::Fail,
            "test_fixture_function_name".to_string(),
            "test_fixture_function_name.py".to_string(),
        );
        let display = SubDiagnosticDisplay::new(&diagnostic);
        assert_eq!(
            display.to_string(),
            "fail[assertion-failed]".red().to_string()
                + " in test_fixture_function_name.py\n | test_fixture_function_name\n"
        );
    }

    #[test]
    fn test_sub_diagnostic_error_display() {
        let diagnostic = SubDiagnostic::new(
            SubDiagnosticType::Error(DiagnosticError::Error("ValueError".to_string())),
            "test_fixture_function_name".to_string(),
            "test_fixture_function_name.py".to_string(),
        );
        let display = SubDiagnosticDisplay::new(&diagnostic);
        assert_eq!(
            display.to_string(),
            "error[value-error]".yellow().to_string()
                + " in test_fixture_function_name.py\n | test_fixture_function_name\n"
        );
    }

    #[test]
    fn test_sub_diagnostic_fixture_not_found_display() {
        let diagnostic =
            Diagnostic::fixture_not_found("fixture_name", "test_fixture_function_name.py");
        assert_eq!(
            diagnostic.display().to_string(),
            "error[fixture-not-found]".yellow().to_string()
                + " in test_fixture_function_name.py\n | Fixture fixture_name not found\n"
        );
    }

    #[test]
    fn test_sub_diagnostic_invalid_fixture_display() {
        let diagnostic =
            Diagnostic::invalid_fixture("fixture_name", "test_fixture_function_name.py");
        assert_eq!(
            diagnostic.display().to_string(),
            "error[invalid-fixture]".yellow().to_string()
                + " in test_fixture_function_name.py\n | fixture_name\n"
        );
    }
}
