use pyo3::prelude::*;

use crate::{
    Context, TestRunResult,
    discovery::{DiscoveredModule, DiscoveredPackage},
    extensions::fixtures::{FixtureManager, FixtureScope},
};

/// Collects and processes test cases from given packages, modules, and test functions.
pub struct DiscoveredPackageRunner<'ctx, 'proj, 'rep> {
    context: &'ctx mut Context<'proj, 'rep>,
}

impl<'ctx, 'proj, 'rep> DiscoveredPackageRunner<'ctx, 'proj, 'rep> {
    pub(crate) const fn new(context: &'ctx mut Context<'proj, 'rep>) -> Self {
        Self { context }
    }

    pub(crate) fn run(&mut self, py: Python<'_>, session: &DiscoveredPackage) {
        tracing::info!("Running discovered test cases");

        let mut fixture_manager = FixtureManager::new();

        fixture_manager.setup_auto_use_fixtures(
            py,
            &[],
            session,
            &FixtureScope::Session.scopes_above(),
        );

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
    ) -> bool {
        fixture_manager.setup_auto_use_fixtures(
            py,
            parents,
            module,
            &FixtureScope::Module.scopes_above(),
        );

        let cleanup = |fixture_manager: &mut FixtureManager, result: &mut TestRunResult| {
            let finalizers = fixture_manager.clear_finalizers(FixtureScope::Module);

            result.add_test_diagnostics(finalizers.run(py));

            fixture_manager.clear_fixtures(FixtureScope::Module);
        };

        let mut passed = true;

        for test_function in module.test_functions() {
            passed &= test_function.run(py, module, parents, fixture_manager, self.context);

            if self.context.project().options().fail_fast() && !passed {
                cleanup(fixture_manager, self.context.result_mut());
                break;
            }
        }

        passed
    }

    fn run_package(
        &mut self,
        py: Python<'_>,
        package: &DiscoveredPackage,
        parents: &[&DiscoveredPackage],
        fixture_manager: &mut FixtureManager,
    ) -> bool {
        fixture_manager.setup_auto_use_fixtures(
            py,
            parents,
            package,
            &FixtureScope::Package.scopes_above(),
        );

        let mut passed = true;

        let mut new_parents = parents.to_vec();

        new_parents.push(package);

        let clean_up = |fixture_manager: &mut FixtureManager, result: &mut TestRunResult| {
            let finalizers = fixture_manager.clear_finalizers(FixtureScope::Package);

            result.add_test_diagnostics(finalizers.run(py));

            fixture_manager.clear_fixtures(FixtureScope::Package);
        };

        for module in package.modules().values() {
            passed &= self.run_module(py, module, &new_parents, fixture_manager);

            if self.context.project().options().fail_fast() && !passed {
                clean_up(fixture_manager, self.context.result_mut());
                break;
            }
        }

        for sub_package in package.packages().values() {
            passed &= self.run_package(py, sub_package, &new_parents, fixture_manager);
        }

        passed
    }
}
