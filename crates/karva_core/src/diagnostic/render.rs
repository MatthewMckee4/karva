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
            SubDiagnosticType::Error(DiagnosticError::Error(type_name)) => {
                format!("error[{}]", to_kebab_case(type_name)).yellow()
            }
            SubDiagnosticType::Error(DiagnosticError::FixtureNotFound(_)) => {
                "error[fixture-not-found]".to_string().yellow()
            }
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
