use pyo3::prelude::*;

use crate::{
    collection::{CollectedModule, CollectedPackage, TestCase},
    discovery::{DiscoveredModule, DiscoveredPackage, TestFunction},
    extensions::fixtures::{FixtureManager, FixtureScope, UsesFixtures},
    utils::{Upcast, iter_with_ancestors},
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

        let upcast_test_cases = session.all_uses_fixtures();

        let mut session_collected = CollectedPackage::default();

        fixture_manager.add_fixtures(
            py,
            &[],
            session,
            &[FixtureScope::Session],
            upcast_test_cases.as_slice(),
        );

        let package_collected = Self::collect_package(py, session, &[], &fixture_manager);

        session_collected.add_finalizers(fixture_manager.reset_fixtures());

        session_collected.add_package(package_collected);

        session_collected
    }

    fn collect_test_function<'a>(
        py: Python<'_>,
        test_function: &'a TestFunction,
        py_module: &Py<PyModule>,
        module: &'a DiscoveredModule,
        parents: &[&DiscoveredPackage],
        fixture_manager: &FixtureManager,
    ) -> Vec<TestCase<'a>> {
        let mut function_fixture_manager =
            FixtureManager::new(Some(fixture_manager), FixtureScope::Function);

        let setup_fixture_manager = |fixture_manager: &mut FixtureManager<'_>| {
            let test_cases = [test_function].to_vec();

            let upcast_test_cases: Vec<&dyn UsesFixtures> = test_cases.upcast();

            for (parent, parents_above_current_parent) in iter_with_ancestors(parents) {
                fixture_manager.add_fixtures(
                    py,
                    &parents_above_current_parent,
                    parent,
                    &[FixtureScope::Function],
                    upcast_test_cases.as_slice(),
                );
            }

            fixture_manager.add_fixtures(
                py,
                parents,
                module,
                &[FixtureScope::Function],
                upcast_test_cases.as_slice(),
            );
        };

        test_function.collect(
            py,
            module,
            py_module,
            &mut function_fixture_manager,
            setup_fixture_manager,
        )
    }

    fn collect_module<'a>(
        py: Python<'_>,
        module: &'a DiscoveredModule,
        parents: &[&DiscoveredPackage],
        fixture_manager: &FixtureManager,
    ) -> CollectedModule<'a> {
        let mut module_collected = CollectedModule::default();
        if module.total_test_functions() == 0 {
            return module_collected;
        }

        let module_test_cases = module.all_uses_fixtures();

        if module_test_cases.is_empty() {
            return module_collected;
        }

        let mut module_fixture_manager =
            FixtureManager::new(Some(fixture_manager), FixtureScope::Module);

        for (parent, parents_above_current_parent) in iter_with_ancestors(parents) {
            module_fixture_manager.add_fixtures(
                py,
                &parents_above_current_parent,
                parent,
                &[FixtureScope::Module],
                module_test_cases.as_slice(),
            );
        }

        module_fixture_manager.add_fixtures(
            py,
            parents,
            module,
            &[
                FixtureScope::Module,
                FixtureScope::Package,
                FixtureScope::Session,
            ],
            module_test_cases.as_slice(),
        );

        let module_name = module.name();

        if module_name.is_empty() {
            return module_collected;
        }

        let Ok(py_module) = PyModule::import(py, module_name) else {
            return module_collected;
        };

        let py_module = py_module.unbind();

        let mut module_test_cases = Vec::new();

        module.test_functions().iter().for_each(|function| {
            module_test_cases.extend(Self::collect_test_function(
                py,
                function,
                &py_module,
                module,
                parents,
                &module_fixture_manager,
            ));
        });

        module_collected.add_test_cases(module_test_cases);

        module_collected.add_finalizers(module_fixture_manager.reset_fixtures());

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

        let package_test_cases = package.all_uses_fixtures();

        let mut package_fixture_manager =
            FixtureManager::new(Some(fixture_manager), FixtureScope::Package);

        for (parent, parents_above_current_parent) in iter_with_ancestors(parents) {
            package_fixture_manager.add_fixtures(
                py,
                &parents_above_current_parent,
                parent,
                &[FixtureScope::Package],
                package_test_cases.as_slice(),
            );
        }

        package_fixture_manager.add_fixtures(
            py,
            parents,
            package,
            &[FixtureScope::Package, FixtureScope::Session],
            package_test_cases.as_slice(),
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

        package_collected
    }
}
