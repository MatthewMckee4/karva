use pyo3::prelude::*;

use crate::{
    collection::CollectedModule, diagnostic::reporter::Reporter, extensions::fixtures::Finalizers,
    runner::RunDiagnostics,
};

#[derive(Default)]
pub struct CollectedPackage<'proj> {
    finalizers: Finalizers,
    modules: Vec<CollectedModule<'proj>>,
    packages: Vec<CollectedPackage<'proj>>,
}

impl<'proj> CollectedPackage<'proj> {
    pub fn add_collected_module(&mut self, collected_module: CollectedModule<'proj>) {
        self.modules.push(collected_module);
    }

    pub fn add_collected_package(&mut self, collected_package: Self) {
        self.packages.push(collected_package);
    }

    pub fn add_finalizers(&mut self, finalizers: Finalizers) {
        self.finalizers.update(finalizers);
    }

    #[must_use]
    pub fn total_test_cases(&self) -> usize {
        let mut total = 0;
        for module in &self.modules {
            total += module.total_test_cases();
        }
        for package in &self.packages {
            total += package.total_test_cases();
        }
        total
    }

    pub fn run_with_reporter(&self, py: Python<'_>, reporter: &mut dyn Reporter) -> RunDiagnostics {
        let mut diagnostics = RunDiagnostics::default();

        for module in &self.modules {
            diagnostics.update(&module.run_with_reporter(py, reporter));
        }

        for package in &self.packages {
            diagnostics.update(&package.run_with_reporter(py, reporter));
        }

        diagnostics.add_diagnostics(self.finalizers.run(py));

        diagnostics
    }
}
