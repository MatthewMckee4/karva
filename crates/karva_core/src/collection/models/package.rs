use pyo3::prelude::*;

use crate::{
    collection::CollectedModule,
    diagnostic::{Diagnostic, reporter::Reporter},
    extensions::fixtures::Finalizers,
    runner::TestRunResult,
};

/// A collected package represents a single Python package with its test cases and finalizers.
#[derive(Default, Debug)]
pub(crate) struct CollectedPackage<'proj> {
    /// The modules in the package.
    modules: Vec<CollectedModule<'proj>>,

    /// The sub-packages in the package.
    packages: Vec<CollectedPackage<'proj>>,

    /// Finalizers to run after the tests are executed.
    finalizers: Finalizers,

    /// Fixture diagnostics collected during the test run.
    fixture_diagnostics: Vec<Diagnostic>,
}

impl<'proj> CollectedPackage<'proj> {
    pub(crate) fn add_module(&mut self, collected_module: CollectedModule<'proj>) {
        self.modules.push(collected_module);
    }

    pub(crate) fn add_package(&mut self, collected_package: Self) {
        self.packages.push(collected_package);
    }

    pub(crate) fn add_finalizers(&mut self, finalizers: Finalizers) {
        self.finalizers.update(finalizers);
    }

    pub(crate) fn add_fixture_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        self.fixture_diagnostics.extend(diagnostics);
    }

    /// Count the number of test cases in the package.
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

    pub(crate) fn run_with_reporter(
        self,
        py: Python<'_>,
        reporter: &dyn Reporter,
    ) -> TestRunResult {
        let Self {
            modules,
            packages,
            finalizers,
            fixture_diagnostics,
        } = self;

        let mut run_result = TestRunResult::default();

        for module in modules {
            run_result.update(module.run_with_reporter(py, reporter));
        }

        for package in packages {
            run_result.update(package.run_with_reporter(py, reporter));
        }

        run_result.add_test_diagnostics(finalizers.run(py));

        run_result.add_test_diagnostics(fixture_diagnostics);

        run_result
    }
}
