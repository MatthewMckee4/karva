use pyo3::prelude::*;

use crate::{
    Context,
    discovery::{DiscoveredModule, DiscoveredPackage},
    extensions::fixtures::{FixtureManager, FixtureScope},
};

/// Collects and processes test cases from given packages, modules, and test functions.
pub(crate) struct DiscoveredPackageRunner<'ctx, 'proj, 'rep> {
    context: &'ctx mut Context<'proj, 'rep>,
}

impl<'ctx, 'proj, 'rep> DiscoveredPackageRunner<'ctx, 'proj, 'rep> {
    pub(crate) const fn new(context: &'ctx mut Context<'proj, 'rep>) -> Self {
        Self { context }
    }

    pub(crate) fn run(&mut self, py: Python<'_>, session: &DiscoveredPackage) {
        tracing::info!("Running discovered test cases");

        let mut fixture_manager = FixtureManager::new();

        self.run_package(py, session, &[], &mut fixture_manager);

        let finalizers = fixture_manager.clear_finalizers(FixtureScope::Session);

        self.context
            .result_mut()
            .add_test_diagnostics(finalizers.run(py));

        self.context
            .result_mut()
            .add_test_diagnostics(fixture_manager.clear_diagnostics());

        fixture_manager.clear_fixtures(FixtureScope::Session);
    }

    fn run_module(
        &mut self,
        py: Python<'_>,
        module: &DiscoveredModule,
        parents: &[&DiscoveredPackage],
        fixture_manager: &mut FixtureManager,
    ) {
        for test_function in module.test_functions() {
            test_function.run(py, module, parents, fixture_manager, self.context);
        }

        let finalizers = fixture_manager.clear_finalizers(FixtureScope::Module);

        self.context
            .result_mut()
            .add_test_diagnostics(finalizers.run(py));

        fixture_manager.clear_fixtures(FixtureScope::Module);
    }

    fn run_package(
        &mut self,
        py: Python<'_>,
        package: &DiscoveredPackage,
        parents: &[&DiscoveredPackage],
        fixture_manager: &mut FixtureManager,
    ) {
        let mut new_parents = parents.to_vec();

        new_parents.push(package);

        for module in package.modules().values() {
            self.run_module(py, module, &new_parents, fixture_manager);
        }

        for sub_package in package.packages().values() {
            self.run_package(py, sub_package, &new_parents, fixture_manager);
        }

        let finalizers = fixture_manager.clear_finalizers(FixtureScope::Package);

        self.context
            .result_mut()
            .add_test_diagnostics(finalizers.run(py));

        fixture_manager.clear_fixtures(FixtureScope::Package);
    }
}
