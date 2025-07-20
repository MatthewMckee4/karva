use pyo3::prelude::*;
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};

use crate::{
    collection::CollectedModule, diagnostic::reporter::Reporter, extensions::fixtures::Finalizers,
    runner::RunDiagnostics,
};

#[derive(Default)]
pub(crate) struct CollectedPackage<'proj> {
    finalizers: Finalizers,
    modules: Vec<CollectedModule<'proj>>,
    packages: Vec<CollectedPackage<'proj>>,
}

impl<'proj> CollectedPackage<'proj> {
    pub(crate) fn add_collected_module(&mut self, collected_module: CollectedModule<'proj>) {
        self.modules.push(collected_module);
    }

    pub(crate) fn add_collected_package(&mut self, collected_package: Self) {
        self.packages.push(collected_package);
    }

    pub(crate) fn add_finalizers(&mut self, finalizers: Finalizers) {
        self.finalizers.update(finalizers);
    }

    #[must_use]
    pub(crate) fn total_test_cases(&self) -> usize {
        let mut total = 0;
        for module in &self.modules {
            total += module.total_test_cases();
        }
        for package in &self.packages {
            total += package.total_test_cases();
        }
        total
    }

    #[must_use]
    pub(crate) fn total_modules(&self) -> usize {
        let mut total = 0;
        for module in &self.modules {
            if !module.test_cases().is_empty() {
                total += 1;
            }
        }
        for package in &self.packages {
            total += package.total_modules();
        }
        total
    }

    pub(crate) fn run_with_reporter(
        &self,
        py: Python<'_>,
        reporter: &dyn Reporter,
    ) -> RunDiagnostics {
        let mut diagnostics = RunDiagnostics::default();

        py.allow_threads(|| {
            self.modules
                .par_iter()
                .map(|module| {
                    Python::with_gil(|inner_py| module.run_with_reporter(inner_py, reporter))
                })
                .collect::<Vec<_>>()
        })
        .iter()
        .for_each(|result| diagnostics.update(result));

        py.allow_threads(|| {
            self.packages
                .par_iter()
                .map(|package| {
                    Python::with_gil(|inner_py| package.run_with_reporter(inner_py, reporter))
                })
                .collect::<Vec<_>>()
        })
        .iter()
        .for_each(|result| diagnostics.update(result));

        diagnostics.add_diagnostics(self.finalizers.run(py));

        diagnostics
    }
}
