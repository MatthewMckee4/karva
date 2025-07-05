use pyo3::prelude::*;

use crate::{
    diagnostic::{Diagnostic, DiagnosticScope, ErrorType, Severity},
    fixture::{FixtureManager, FixtureScope, RequiresFixtures},
    models::{Module, Package, TestCase},
    utils::Upcast,
};

mod diagnostic;

use diagnostic::CollectorDiagnostics;

pub struct TestCaseCollector<'proj> {
    session: &'proj Package<'proj>,
}

impl<'proj> TestCaseCollector<'proj> {
    #[must_use]
    pub const fn new(session: &'proj Package<'proj>) -> Self {
        Self { session }
    }

    #[must_use]
    pub fn collect(&self, py: Python<'_>) -> CollectorDiagnostics<'proj> {
        let mut diagnostics = CollectorDiagnostics::default();

        let mut fixture_manager = FixtureManager::new();
        let upcast_test_cases: Vec<&dyn RequiresFixtures> = self.session.test_cases().upcast();

        fixture_manager.add_fixtures(
            py,
            &[],
            self.session,
            &[FixtureScope::Session],
            upcast_test_cases.as_slice(),
        );

        let mut package_diagnostics =
            self.collect_package(py, self.session, &[], &mut fixture_manager);

        package_diagnostics.add_diagnostics(fixture_manager.reset_session_fixtures(py));

        diagnostics.update(package_diagnostics);

        diagnostics
    }

    #[allow(clippy::unused_self)]
    fn collect_module(
        &self,
        py: Python<'_>,
        module: &'proj Module<'proj>,
        parents: &[&'proj Package<'proj>],
        fixture_manager: &mut FixtureManager,
    ) -> CollectorDiagnostics<'proj> {
        let mut diagnostics = CollectorDiagnostics::default();
        if module.total_test_cases() == 0 {
            return diagnostics;
        }

        let module_test_cases = module.dependencies();
        let upcast_module_test_cases: Vec<&dyn RequiresFixtures> = module_test_cases.upcast();
        if upcast_module_test_cases.is_empty() {
            return diagnostics;
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

        let py_module = match PyModule::import(py, module.name()) {
            Ok(py_module) => py_module,
            Err(err) => {
                diagnostics.add_diagnostic(Diagnostic::from_py_err(
                    py,
                    &err,
                    DiagnosticScope::Setup,
                    Some(module.path().to_string()),
                    Severity::Error(ErrorType::Unknown),
                ));
                return diagnostics;
            }
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

                    let result = f(fixture_manager);

                    diagnostics.add_diagnostics(fixture_manager.reset_function_fixtures(py));

                    result
                };

            let result = function.collect(py, &py_module, &mut get_function_fixture_manager);

            match result {
                Ok(test_case_results) => {
                    for test_case_result in test_case_results {
                        match test_case_result {
                            Ok(test_case) => {
                                diagnostics.add_test_case(test_case);
                            }
                            Err(diagnostic) => {
                                diagnostics.add_diagnostic(diagnostic);
                            }
                        }
                    }
                }
                Err(diagnostic) => {
                    diagnostics.add_diagnostic(diagnostic);
                }
            }
        }

        diagnostics.add_diagnostics(fixture_manager.reset_module_fixtures(py));

        diagnostics
    }

    fn collect_package(
        &self,
        py: Python<'_>,
        package: &'proj Package<'proj>,
        parents: &[&'proj Package<'proj>],
        fixture_manager: &mut FixtureManager,
    ) -> CollectorDiagnostics<'proj> {
        let mut package_diagnostics = CollectorDiagnostics::default();
        if package.total_test_cases() == 0 {
            return package_diagnostics;
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

        let module_diagnostics = {
            package
                .modules()
                .values()
                .map(|module| self.collect_module(py, module, &new_parents, fixture_manager))
                .collect::<Vec<_>>()
        };

        for module_diagnostics in module_diagnostics {
            package_diagnostics.update(module_diagnostics);
        }

        for sub_package in package.packages().values() {
            package_diagnostics.update(self.collect_package(
                py,
                sub_package,
                &new_parents,
                fixture_manager,
            ));
        }
        package_diagnostics.add_diagnostics(fixture_manager.reset_package_fixtures(py));

        package_diagnostics
    }
}
