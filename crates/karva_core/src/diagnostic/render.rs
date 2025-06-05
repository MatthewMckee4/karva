use colored::Colorize;

use crate::diagnostic::{Diagnostic, DiagnosticType};

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
        match self.diagnostic.diagnostic_type() {
            DiagnosticType::Fail => {
                writeln!(f, "{}", "fail[assertion-failed]".red())?;
            }
            DiagnosticType::Error(type_name) => {
                writeln!(
                    f,
                    "{}",
                    format!("error[{}]", to_kebab_case(type_name)).yellow()
                )?;
            }
        }

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
