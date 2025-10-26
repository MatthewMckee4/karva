use pyo3::prelude::*;

use crate::{
    collection::CollectedModule, diagnostic::reporter::Reporter, extensions::fixtures::Finalizers,
    runner::TestRunResult,
};

/// A collected package represents a single Python package with its test cases and finalizers.
#[derive(Default, Debug)]
pub(crate) struct CollectedPackage<'proj> {
    /// The modules in the package.
    modules: Vec<CollectedModule<'proj>>,

    /// The sub-packages in the package.
    packages: Vec<CollectedPackage<'proj>>,

    /// Finalizers to run after the package is executed.
    finalizers: Finalizers,
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

    /// Count the number of test cases in the package.
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

    /// Count the number of modules in the package that have test cases.
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
    ) -> TestRunResult {
        let mut diagnostics = TestRunResult::default();

        self.modules
            .iter()
            .for_each(|module| diagnostics.update(&module.run_with_reporter(py, reporter)));

        self.packages
            .iter()
            .for_each(|package| diagnostics.update(&package.run_with_reporter(py, reporter)));

        diagnostics.add_diagnostics(self.finalizers.run(py));

        diagnostics
    }
}
