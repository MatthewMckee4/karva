use crate::{diagnostic::Diagnostic, models::TestCase, runner::RunDiagnostics};

#[derive(Debug, Default)]
pub struct CollectorDiagnostics<'proj> {
    diagnostics: RunDiagnostics,
    test_cases: Vec<TestCase<'proj>>,
}

impl<'proj> CollectorDiagnostics<'proj> {
    pub fn add_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        self.diagnostics.add_diagnostics(diagnostics);
    }

    pub fn add_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.add_diagnostic(diagnostic);
    }

    pub fn add_test_case(&mut self, test_case: TestCase<'proj>) {
        self.test_cases.push(test_case);
    }

    pub fn update(&mut self, other: Self) {
        self.diagnostics.update(&other.diagnostics);
        self.test_cases.extend(other.test_cases);
    }

    pub fn test_cases(&self) -> &[TestCase<'proj>] {
        &self.test_cases
    }

    pub const fn diagnostics(&self) -> &RunDiagnostics {
        &self.diagnostics
    }
}
