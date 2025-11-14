use pyo3::prelude::*;

use crate::{
    collection::{CollectedModule, CollectedPackage, TestCase},
    discovery::{DiscoveredModule, DiscoveredPackage, TestFunction},
    extensions::fixtures::{FixtureManager, FixtureScope, RequiresFixtures},
};

/// Collects and processes test cases from given packages, modules, and test functions.
pub(crate) struct TestCaseCollector;

impl TestCaseCollector {
    pub(crate) fn collect<'a>(
        py: Python<'_>,
        session: &'a DiscoveredPackage,
    ) -> CollectedPackage<'a> {
        tracing::info!("Collecting test cases");

        let mut fixture_manager = FixtureManager::new(None, FixtureScope::Session);

        let required_session_fixture_names = session.required_fixtures(py);

        fixture_manager.add_fixtures(
            py,
            &[],
            &session,
            &[FixtureScope::Session],
            &required_session_fixture_names,
        );

        let mut session_collected = Self::collect_package(py, session, &[], &fixture_manager);

        session_collected.add_finalizers(fixture_manager.reset_fixtures());

        session_collected.add_fixture_diagnostics(fixture_manager.clear_diagnostics());

        session_collected
    }

    fn collect_test_function<'a>(
        py: Python<'_>,
        test_function: &'a TestFunction,
        module: &'a DiscoveredModule,
        parents: &[&DiscoveredPackage],
        fixture_manager: &FixtureManager,
    ) -> Vec<TestCase<'a>> {
        let get_fixture_manager = || {
            let required_fixtures = test_function.required_fixtures(py);

            FixtureManager::from_parent(
                py,
                fixture_manager,
                parents,
                module,
                FixtureScope::Function,
                &required_fixtures,
            )
        };

        test_function.collect(py, module, get_fixture_manager)
    }

    fn collect_module<'a>(
        py: Python<'_>,
        module: &'a DiscoveredModule,
        parents: &[&DiscoveredPackage],
        parent_fixture_manager: &FixtureManager,
    ) -> CollectedModule<'a> {
        let mut module_collected = CollectedModule::default();
        if module.total_test_functions() == 0 {
            return module_collected;
        }

        let required_fixtures = module.required_fixtures(py);

        let mut module_fixture_manager = FixtureManager::from_parent(
            py,
            parent_fixture_manager,
            parents,
            module,
            FixtureScope::Module,
            &required_fixtures,
        );

        let mut module_test_cases = Vec::new();

        module.test_functions().iter().for_each(|function| {
            module_test_cases.extend(Self::collect_test_function(
                py,
                function,
                module,
                parents,
                &module_fixture_manager,
            ));
        });

        module_collected.add_test_cases(module_test_cases);

        module_collected.add_finalizers(module_fixture_manager.reset_fixtures());

        module_collected.add_fixture_diagnostics(module_fixture_manager.clear_diagnostics());

        module_collected
    }

    fn collect_package<'a>(
        py: Python<'_>,
        package: &'a DiscoveredPackage,
        parents: &[&DiscoveredPackage],
        fixture_manager: &FixtureManager,
    ) -> CollectedPackage<'a> {
        let mut package_collected = CollectedPackage::default();

        if package.total_test_functions() == 0 {
            return package_collected;
        }

        let required_fixtures = package.required_fixtures(py);

        let mut package_fixture_manager = FixtureManager::from_parent(
            py,
            fixture_manager,
            parents,
            package,
            FixtureScope::Package,
            &required_fixtures,
        );

        let mut new_parents = parents.to_vec();

        new_parents.push(package);

        for module in package.modules().values() {
            let module_collected =
                Self::collect_module(py, module, &new_parents, &package_fixture_manager);
            package_collected.add_module(module_collected);
        }

        for sub_package in package.packages().values() {
            let sub_package_collected =
                Self::collect_package(py, sub_package, &new_parents, &package_fixture_manager);
            package_collected.add_package(sub_package_collected);
        }

        package_collected.add_finalizers(package_fixture_manager.reset_fixtures());

        package_collected.add_fixture_diagnostics(package_fixture_manager.clear_diagnostics());

        package_collected
    }
}
