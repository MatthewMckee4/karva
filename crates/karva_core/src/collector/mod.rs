use pyo3::prelude::*;

use crate::{
    diagnostic::Diagnostic,
    fixture::{FixtureManager, FixtureScope, RequiresFixtures},
    models::{Module, Package, TestCase},
    utils::Upcast,
};

mod diagnostic;

use diagnostic::CollectorResult;

pub struct TestCaseCollector<'proj> {
    session: &'proj Package<'proj>,
}

impl<'proj> TestCaseCollector<'proj> {
    #[must_use]
    pub const fn new(session: &'proj Package<'proj>) -> Self {
        Self { session }
    }

    #[must_use]
    pub fn collect(&self, py: Python<'_>) -> CollectorResult<'proj> {
        tracing::info!("Collecting test cases");

        let mut result = CollectorResult::default();

        let mut fixture_manager = FixtureManager::new();
        let upcast_test_cases: Vec<&dyn RequiresFixtures> = self.session.test_cases().upcast();

        fixture_manager.add_fixtures(
            py,
            &[],
            self.session,
            &[FixtureScope::Session],
            upcast_test_cases.as_slice(),
        );

        result.update(self.collect_package(py, self.session, &[], &mut fixture_manager));

        result.add_finalizers(fixture_manager.reset_session_fixtures());

        result
    }

    #[allow(clippy::unused_self)]
    fn collect_module(
        &self,
        py: Python<'_>,
        module: &'proj Module<'proj>,
        parents: &[&'proj Package<'proj>],
        fixture_manager: &mut FixtureManager,
    ) -> CollectorResult<'proj> {
        let mut result = CollectorResult::default();
        if module.total_test_cases() == 0 {
            return result;
        }

        let module_test_cases = module.dependencies();
        let upcast_module_test_cases: Vec<&dyn RequiresFixtures> = module_test_cases.upcast();
        if upcast_module_test_cases.is_empty() {
            return result;
        }

        let mut parents_above_current_parent = parents.to_vec();
        let mut i = parents.len();
        while i > 0 {
            i -= 1;
            let parent = parents[i];
            parents_above_current_parent.truncate(i);
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
            return result;
        };

        for function in module.test_cases() {
            let mut get_function_fixture_manager =
                |f: &dyn Fn(&FixtureManager) -> Result<TestCase<'proj>, Diagnostic>| {
                    let test_cases = [function].to_vec();
                    let upcast_test_cases: Vec<&dyn RequiresFixtures> = test_cases.upcast();

                    let mut parents_above_current_parent = parents.to_vec();
                    let mut i = parents.len();
                    while i > 0 {
                        i -= 1;
                        let parent = parents[i];
                        parents_above_current_parent.truncate(i);
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

                    let collected_test_case = f(fixture_manager);

                    result.add_finalizers(fixture_manager.reset_function_fixtures());

                    collected_test_case
                };

            let collected_test_cases =
                function.collect(py, &py_module, &mut get_function_fixture_manager);

            for test_case_result in collected_test_cases {
                match test_case_result {
                    Ok(test_case) => {
                        result.add_test_case(test_case);
                    }
                    Err(diagnostic) => {
                        result.add_diagnostic(diagnostic);
                    }
                }
            }
        }

        result.add_finalizers(fixture_manager.reset_module_fixtures());

        result
    }

    fn collect_package(
        &self,
        py: Python<'_>,
        package: &'proj Package<'proj>,
        parents: &[&'proj Package<'proj>],
        fixture_manager: &mut FixtureManager,
    ) -> CollectorResult<'proj> {
        let mut result = CollectorResult::default();
        if package.total_test_cases() == 0 {
            return result;
        }
        let package_test_cases = package.dependencies();

        let upcast_package_test_cases: Vec<&dyn RequiresFixtures> = package_test_cases.upcast();

        let mut parents_above_current_parent = parents.to_vec();
        let mut i = parents.len();
        while i > 0 {
            i -= 1;
            let parent = parents[i];
            parents_above_current_parent.truncate(i);
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

        package.modules().values().for_each(|module| {
            let module_diagnostics = self.collect_module(py, module, &new_parents, fixture_manager);
            result.update(module_diagnostics);
        });

        for sub_package in package.packages().values() {
            result.update(self.collect_package(py, sub_package, &new_parents, fixture_manager));
        }
        result.add_finalizers(fixture_manager.reset_package_fixtures());

        result
    }
}
