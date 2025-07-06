use pyo3::prelude::*;

use crate::{
    diagnostic::Diagnostic,
    fixture::{FixtureManager, FixtureScope, RequiresFixtures},
    models::{
        Module, Package, TestCase, TestFunction, module::CollectedModule, package::CollectedPackage,
    },
    utils::{Upcast, partition_iter},
};

#[derive(Default)]
pub struct TestCaseCollector;

impl TestCaseCollector {
    #[must_use]
    pub fn collect<'a>(
        py: Python<'_>,
        session: &'a Package<'a>,
    ) -> (CollectedPackage<'a>, Vec<Diagnostic>) {
        tracing::info!("Collecting test cases");

        let mut result = Vec::new();

        let mut fixture_manager = FixtureManager::new();

        let upcast_test_cases: Vec<&dyn RequiresFixtures> = session.test_functions().upcast();

        let mut session_collected = CollectedPackage::default();

        fixture_manager.add_fixtures(
            py,
            &[],
            session,
            &[FixtureScope::Session],
            upcast_test_cases.as_slice(),
        );

        let (package_collected, package_diagnostics) =
            Self::collect_package(py, session, &[], &mut fixture_manager);

        result.extend(package_diagnostics);

        session_collected.add_finalizers(fixture_manager.reset_session_fixtures());

        session_collected.add_collected_package(package_collected);

        (session_collected, result)
    }

    fn collect_test_function<'a>(
        py: Python<'_>,
        test_function: &'a TestFunction<'a>,
        py_module: &Bound<'_, PyModule>,
        module: &'a Module<'a>,
        parents: &[&Package<'_>],
        fixture_manager: &mut FixtureManager,
    ) -> (Vec<TestCase<'a>>, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();

        let mut test_cases = Vec::new();

        let mut get_function_fixture_manager =
            |f: &mut dyn FnMut(&mut FixtureManager) -> Result<TestCase<'a>, Diagnostic>| {
                let test_cases = [test_function].to_vec();
                let upcast_test_cases: Vec<&dyn RequiresFixtures> = test_cases.upcast();

                for (parent, parents_above_current_parent) in partition_iter(parents) {
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

                let mut collected_test_case = f(fixture_manager);

                if let Ok(test_case) = &mut collected_test_case {
                    test_case.add_finalizers(fixture_manager.reset_function_fixtures());
                }

                collected_test_case
            };

        let collected_test_cases =
            test_function.collect(py, py_module, &mut get_function_fixture_manager);

        for test_case_result in collected_test_cases {
            match test_case_result {
                Ok(test_case) => {
                    test_cases.push(test_case);
                }
                Err(diagnostic) => {
                    diagnostics.push(diagnostic);
                }
            }
        }

        (test_cases, diagnostics)
    }

    #[allow(clippy::unused_self)]
    fn collect_module<'a>(
        py: Python<'_>,
        module: &'a Module<'a>,
        parents: &[&Package<'_>],
        fixture_manager: &mut FixtureManager,
    ) -> (CollectedModule<'a>, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        let mut module_collected = CollectedModule::default();
        if module.total_test_functions() == 0 {
            return (module_collected, diagnostics);
        }

        let module_test_cases = module.dependencies();
        let upcast_module_test_cases: Vec<&dyn RequiresFixtures> = module_test_cases.upcast();
        if upcast_module_test_cases.is_empty() {
            return (module_collected, diagnostics);
        }

        for (parent, parents_above_current_parent) in partition_iter(parents) {
            fixture_manager.add_fixtures(
                py,
                &parents_above_current_parent,
                parent,
                &[FixtureScope::Module],
                upcast_module_test_cases.as_slice(),
            );
        }

        fixture_manager.add_fixtures(
            py,
            parents,
            module,
            &[
                FixtureScope::Module,
                FixtureScope::Package,
                FixtureScope::Session,
            ],
            upcast_module_test_cases.as_slice(),
        );

        let Ok(py_module) = PyModule::import(py, module.name()) else {
            return (module_collected, diagnostics);
        };

        let mut module_test_cases = Vec::new();

        for function in module.test_functions() {
            let (function_test_cases, function_diagnostics) = Self::collect_test_function(
                py,
                function,
                &py_module,
                module,
                parents,
                fixture_manager,
            );
            module_test_cases.extend(function_test_cases);
            diagnostics.extend(function_diagnostics);
        }

        module_collected.add_test_cases(module_test_cases);
        module_collected.add_finalizers(fixture_manager.reset_module_fixtures());

        (module_collected, diagnostics)
    }

    fn collect_package<'a>(
        py: Python<'_>,
        package: &'a Package<'a>,
        parents: &[&Package<'_>],
        fixture_manager: &mut FixtureManager,
    ) -> (CollectedPackage<'a>, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        let mut package_collected = CollectedPackage::default();
        if package.total_test_functions() == 0 {
            return (package_collected, diagnostics);
        }
        let package_test_cases = package.dependencies();

        let upcast_package_test_cases: Vec<&dyn RequiresFixtures> = package_test_cases.upcast();

        for (parent, parents_above_current_parent) in partition_iter(parents) {
            fixture_manager.add_fixtures(
                py,
                &parents_above_current_parent,
                parent,
                &[FixtureScope::Package],
                upcast_package_test_cases.as_slice(),
            );
        }

        fixture_manager.add_fixtures(
            py,
            parents,
            package,
            &[FixtureScope::Package, FixtureScope::Session],
            upcast_package_test_cases.as_slice(),
        );

        let mut new_parents = parents.to_vec();
        new_parents.push(package);

        for module in package.modules().values() {
            let (module_collected, module_diagnostics) =
                Self::collect_module(py, module, &new_parents, fixture_manager);
            diagnostics.extend(module_diagnostics);
            package_collected.add_collected_module(module_collected);
        }

        for sub_package in package.packages().values() {
            let (sub_package_collected, sub_package_diagnostics) =
                Self::collect_package(py, sub_package, &new_parents, fixture_manager);
            diagnostics.extend(sub_package_diagnostics);
            package_collected.add_collected_package(sub_package_collected);
        }

        package_collected.add_finalizers(fixture_manager.reset_package_fixtures());

        (package_collected, diagnostics)
    }
}
