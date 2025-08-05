use pyo3::prelude::*;

use crate::{
    collection::{CollectedModule, CollectedPackage, TestCase},
    diagnostic::Diagnostic,
    discovery::{DiscoveredModule, DiscoveredPackage, TestFunction},
    extensions::fixtures::{FixtureManager, FixtureScope, UsesFixtures},
    utils::{Upcast, create_hierarchy_iterator},
};

/// Collects and processes test cases from discovered test functions.
///
/// The `TestCaseCollector` is responsible for the complex process of converting
/// discovered test functions into executable test cases. This involves resolving
/// fixture dependencies, setting up the proper scope hierarchy, and handling
/// test parametrization.
pub(crate) struct TestCaseCollector;

impl TestCaseCollector {
    /// Collects all test cases from a discovered test session.
    ///
    /// This is the main entry point for test collection. It creates a session-level
    /// fixture manager and recursively processes all packages, modules, and test functions.
    #[must_use]
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

        session_collected.add_collected_package(package_collected);

        session_collected
    }

    /// Collects test cases from a single test function with fixture resolution.
    ///
    /// This method creates a function-scoped fixture manager and delegates to the
    /// test function's collect method. It handles the complex fixture hierarchy
    /// setup required for proper dependency injection.
    #[allow(clippy::unused_self)]
    fn collect_test_function<'a>(
        py: Python<'_>,
        test_function: &'a TestFunction,
        py_module: &Py<PyModule>,
        module: &'a DiscoveredModule,
        parents: &[&DiscoveredPackage],
        fixture_manager: &FixtureManager,
    ) -> Vec<(TestCase<'a>, Option<Diagnostic>)> {
        // Create a closure that sets up function-scoped fixtures for test execution
        let create_function_fixture_manager = |inner_py: Python<'_>,
                                               test_case_creator: &dyn Fn(
            &FixtureManager,
        ) -> (
            TestCase<'a>,
            Option<Diagnostic>,
        )| {
            let mut function_fixture_manager =
                FixtureManager::new(Some(fixture_manager), FixtureScope::Function);
            let test_cases = [test_function].to_vec();
            let upcast_test_cases: Vec<&dyn UsesFixtures> = test_cases.upcast();

            for (parent, parents_above_current_parent) in create_hierarchy_iterator(parents) {
                function_fixture_manager.add_fixtures(
                    inner_py,
                    &parents_above_current_parent,
                    parent,
                    &[FixtureScope::Function],
                    upcast_test_cases.as_slice(),
                );
            }

            function_fixture_manager.add_fixtures(
                inner_py,
                parents,
                module,
                &[FixtureScope::Function],
                upcast_test_cases.as_slice(),
            );

            let (mut collected_test_case, diagnostic) =
                test_case_creator(&function_fixture_manager);

            collected_test_case.add_finalizers(function_fixture_manager.reset_fixtures());

            (collected_test_case, diagnostic)
        };

        test_function.collect(py, module, py_module, create_function_fixture_manager)
    }

    #[allow(clippy::unused_self)]
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

        for (parent, parents_above_current_parent) in create_hierarchy_iterator(parents) {
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

        // If the __file__ attribute from the Python module and the module.file() don't match, add a warning
        let py_module_file = py_module.getattr("__file__");
        if let Ok(py_file) = py_module_file {
            if let Ok(py_file_str) = py_file.extract::<&str>() {
                let rust_module_file = module.path().display().to_string();
                if py_file_str != rust_module_file {
                    module_collected.add_diagnostic(Diagnostic::warning(
                        "ModuleFileMismatch",
                        Some(format!(
                            "Imported module from {py_file_str:?} does not match the discovered module file {rust_module_file:?}"
                        )),
                        Some(rust_module_file),
                    ));
                }
            }
        }

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

        for (parent, parents_above_current_parent) in create_hierarchy_iterator(parents) {
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
            package_collected.add_collected_module(module_collected);
        }

        for sub_package in package.packages().values() {
            let sub_package_collected =
                Self::collect_package(py, sub_package, &new_parents, &package_fixture_manager);
            package_collected.add_collected_package(sub_package_collected);
        }

        package_collected.add_finalizers(package_fixture_manager.reset_fixtures());

        package_collected
    }
}
