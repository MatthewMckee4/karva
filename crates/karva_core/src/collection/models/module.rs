use pyo3::prelude::*;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

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

        py.allow_threads(|| {
            self.test_cases
                .par_iter()
                .map(|(test_case, diagnostic)| {
                    Python::with_gil(|inner_py| {
                        let mut result = test_case.run(inner_py, diagnostic.clone());
                        result.add_diagnostics(test_case.finalizers().run(inner_py));
                        result
                    })
                })
                .collect::<Vec<_>>()
        })
        .iter()
        .for_each(|result| diagnostics.update(result));

        reporter.report();

        diagnostics.add_diagnostics(self.finalizers().run(py));

        diagnostics
    }
}
