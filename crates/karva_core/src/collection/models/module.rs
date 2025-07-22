use pyo3::prelude::*;

use crate::{
    collection::TestCase,
    diagnostic::{Diagnostic, reporter::Reporter},
    extensions::fixtures::Finalizers,
    runner::RunDiagnostics,
};

#[derive(Default)]
pub(crate) struct CollectedModule<'proj> {
    test_cases: Vec<(TestCase<'proj>, Option<Diagnostic>)>,
    finalizers: Finalizers,
}

impl<'proj> CollectedModule<'proj> {
    #[must_use]
    pub(crate) fn total_test_cases(&self) -> usize {
        self.test_cases.len()
    }

    #[must_use]
    pub(crate) const fn test_cases(&self) -> &Vec<(TestCase<'proj>, Option<Diagnostic>)> {
        &self.test_cases
    }

    pub(crate) fn add_test_cases(
        &mut self,
        test_cases: Vec<(TestCase<'proj>, Option<Diagnostic>)>,
    ) {
        self.test_cases.extend(test_cases);
    }

    #[must_use]
    pub(crate) const fn finalizers(&self) -> &Finalizers {
        &self.finalizers
    }

    pub(crate) fn add_finalizers(&mut self, finalizers: Finalizers) {
        self.finalizers.update(finalizers);
    }

    pub(crate) fn run_with_reporter(
        &self,
        py: Python<'_>,
        reporter: &dyn Reporter,
    ) -> RunDiagnostics {
        let mut diagnostics = RunDiagnostics::default();

        if self.test_cases.is_empty() {
            return diagnostics;
        }

        self.test_cases.iter().for_each(|(test_case, diagnostic)| {
            let mut result = test_case.run(py, diagnostic.clone());
            result.add_diagnostics(test_case.finalizers().run(py));
            diagnostics.update(&result);
        });

        reporter.report();

        diagnostics.add_diagnostics(self.finalizers().run(py));

        diagnostics
    }
}
