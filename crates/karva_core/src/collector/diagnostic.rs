use crate::{diagnostic::Diagnostic, models::TestCase};

#[derive(Debug, Default)]
pub struct CollectorDiagnostics<'proj> {
    diagnostics: Vec<Diagnostic>,
    test_cases: Vec<TestCase<'proj>>,
}

impl<'proj> CollectorDiagnostics<'proj> {
    pub fn add_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        for diagnostic in diagnostics {
            self.diagnostics.push(diagnostic);
        }
    }

    pub fn add_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn add_test_case(&mut self, test_case: TestCase<'proj>) {
        self.test_cases.push(test_case);
    }

    pub fn update(&mut self, other: Self) {
        for diagnostic in other.diagnostics {
            self.diagnostics.push(diagnostic);
        }
        self.test_cases.extend(other.test_cases);
    }

    pub fn test_cases(&self) -> &[TestCase<'proj>] {
        &self.test_cases
    }

    pub const fn diagnostics(&self) -> &Vec<Diagnostic> {
        &self.diagnostics
    }
}
