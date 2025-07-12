use pyo3::prelude::*;

use crate::{
    collection::TestCase, diagnostic::reporter::Reporter, fixture::Finalizers,
    runner::RunDiagnostics,
};

#[derive(Default)]
pub struct CollectedModule<'proj> {
    test_cases: Vec<TestCase<'proj>>,
    finalizers: Finalizers,
}

impl<'proj> CollectedModule<'proj> {
    #[must_use]
    pub fn test_cases(&self) -> &[TestCase<'proj>] {
        &self.test_cases
    }

    pub fn add_test_cases(&mut self, test_cases: Vec<TestCase<'proj>>) {
        self.test_cases.extend(test_cases);
    }

    #[must_use]
    pub const fn finalizers(&self) -> &Finalizers {
        &self.finalizers
    }

    pub fn add_finalizers(&mut self, finalizers: Finalizers) {
        self.finalizers.update(finalizers);
    }

    pub fn run_with_reporter(&self, py: Python<'_>, reporter: &mut dyn Reporter) -> RunDiagnostics {
        let mut diagnostics = RunDiagnostics::default();

        for test_case in &self.test_cases {
            let result = test_case.run(py);
            reporter.report();
            diagnostics.update(&result);
            diagnostics.add_diagnostics(test_case.finalizers().run(py));
        }

        diagnostics.add_diagnostics(self.finalizers().run(py));

        diagnostics
    }
}
