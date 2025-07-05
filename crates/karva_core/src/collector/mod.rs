use pyo3::prelude::*;

use crate::{
    diagnostic::{Diagnostic, DiagnosticScope, ErrorType, Severity, reporter::Reporter},
    fixture::{FixtureManager, FixtureScope, RequiresFixtures},
    models::{Module, Package, TestCase},
    utils::Upcast,
};

mod diagnostic;

use diagnostic::CollectorDiagnostics;

#[derive(Default)]
pub struct TestCaseCollector;

impl TestCaseCollector {
    pub fn collect<'a>(
        &self,
        py: Python<'a>,
        session: &'a Package,
        reporter: &mut dyn Reporter,
    ) -> CollectorDiagnostics<'a> {
        let total_files = session.total_test_modules();

        let total_test_cases = session.total_test_cases();

        tracing::info!(
            "Collected {} test{} in {} file{}",
            total_test_cases,
            if total_test_cases == 1 { "" } else { "s" },
            total_files,
            if total_files == 1 { "" } else { "s" }
        );

        reporter.set(total_files);

        let mut diagnostics = CollectorDiagnostics::default();

        let mut fixture_manager = FixtureManager::new();
        let upcast_test_cases: Vec<&dyn RequiresFixtures> = session.test_cases().upcast();

        fixture_manager.add_fixtures(
            py,
            &[],
            &session,
            &[FixtureScope::Session],
            upcast_test_cases.as_slice(),
        );

        let mut package_diagnostics =
            self.test_package(py, session, &[], &mut fixture_manager, reporter);

        package_diagnostics.add_diagnostics(fixture_manager.reset_session_fixtures(py));

        diagnostics.update(package_diagnostics);

        diagnostics
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::unused_self)]
    fn test_module<'a>(
        &self,
        py: Python<'_>,
        module: &'a Module<'a>,
        parents: &[&'a Package<'a>],
        fixture_manager: &mut FixtureManager,
        reporter: &dyn Reporter,
    ) -> CollectorDiagnostics<'a> {
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
                |f: &dyn Fn(&FixtureManager) -> Result<TestCase<'a>, Diagnostic>| {
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

        reporter.report();

        diagnostics
    }

    fn test_package<'a>(
        &self,
        py: Python<'_>,
        package: &'a Package<'a>,
        parents: &[&'a Package<'a>],
        fixture_manager: &mut FixtureManager,
        reporter: &dyn Reporter,
    ) -> CollectorDiagnostics<'a> {
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
                .map(|module| self.test_module(py, module, &new_parents, fixture_manager, reporter))
                .collect::<Vec<_>>()
        };

        for module_diagnostics in module_diagnostics {
            package_diagnostics.update(module_diagnostics);
        }

        for sub_package in package.packages().values() {
            package_diagnostics.update(self.test_package(
                py,
                sub_package,
                &new_parents,
                fixture_manager,
                reporter,
            ));
        }
        package_diagnostics.add_diagnostics(fixture_manager.reset_package_fixtures(py));

        package_diagnostics
    }
}
