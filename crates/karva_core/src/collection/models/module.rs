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

        for (test_case, diagnostic) in &self.test_cases {
            let result = test_case.run(py, diagnostic.clone());
            reporter.report();
            diagnostics.update(&result);
            diagnostics.add_diagnostics(test_case.finalizers().run(py));
        }

        diagnostics.add_diagnostics(self.finalizers().run(py));

        diagnostics
    }
}
