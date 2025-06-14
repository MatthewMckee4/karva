use std::collections::HashMap;

use karva_project::project::Project;
use pyo3::prelude::*;

use crate::{
    diagnostic::{
        Diagnostic, DiagnosticScope,
        reporter::{DummyReporter, Reporter},
    },
    discovery::Discoverer,
    fixture::{CalledFixtures, FixtureScope, HasFixtures, TestCaseFixtures},
    module::Module,
    package::Package,
    utils::add_to_sys_path,
};

mod diagnostic;

pub use diagnostic::RunDiagnostics;

pub trait TestRunner {
    fn test(&self) -> RunDiagnostics;
    fn test_with_reporter(&self, reporter: &mut dyn Reporter) -> RunDiagnostics;
}

pub struct StandardTestRunner<'proj> {
    project: &'proj Project,
}

impl<'proj> StandardTestRunner<'proj> {
    #[must_use]
    pub const fn new(project: &'proj Project) -> Self {
        Self { project }
    }

    fn test_impl(&self, reporter: &mut dyn Reporter) -> RunDiagnostics {
        let (session, discovery_diagnostics) = Discoverer::new(self.project).discover();

        let total_files = session.total_test_modules();

        let total_test_cases = session.total_test_cases();

        tracing::info!(
            "Discovered {} tests in {} files",
            total_test_cases,
            total_files
        );

        reporter.set(total_files);

        let mut diagnostics = Vec::new();

        diagnostics.extend(discovery_diagnostics);
        Python::with_gil(|py| {
            let cwd = self.project.cwd();

            if let Err(err) = add_to_sys_path(&py, cwd) {
                diagnostics.push(Diagnostic::from_py_err(
                    py,
                    &err,
                    DiagnosticScope::Setup,
                    &cwd.to_string(),
                ));
                return;
            }

            let session_fixtures =
                session.called_fixtures(py, &[FixtureScope::Session], &session.test_cases());

            self.test_package(
                py,
                &session,
                &[],
                &session_fixtures,
                &mut diagnostics,
                reporter,
            );
        });

        RunDiagnostics {
            diagnostics,
            total_tests: total_test_cases,
        }
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::unused_self)]
    fn test_module<'a>(
        &self,
        py: Python<'a>,
        module: &'a Module<'a>,
        package: &'a Package<'a>,
        parents: &'a [&'a Package<'a>],
        parent_fixtures: &'a CalledFixtures<'a>,
        diagnostics: &mut Vec<Diagnostic>,
        reporter: &dyn Reporter,
    ) {
        if module.total_test_cases() == 0 {
            return;
        }

        let mut current_module_fixtures: CalledFixtures<'a> = HashMap::new();

        let module_fixtures = module.called_fixtures(
            py,
            &[
                FixtureScope::Module,
                FixtureScope::Package,
                FixtureScope::Session,
            ],
            &module.test_cases(),
        );
        for (name, fixture) in module_fixtures {
            current_module_fixtures.insert(name, fixture);
        }

        let package_fixtures =
            package.called_fixtures(py, &[FixtureScope::Module], &module.test_cases());
        for (name, fixture) in package_fixtures {
            current_module_fixtures.insert(name, fixture);
        }

        for parent in parents {
            let parent_fixtures =
                parent.called_fixtures(py, &[FixtureScope::Module], &module.test_cases());
            for (name, fixture) in parent_fixtures {
                current_module_fixtures.insert(name, fixture);
            }
        }

        for (name, fixture) in parent_fixtures {
            current_module_fixtures.insert(name.clone(), fixture.clone());
        }

        let py_module = match PyModule::import(py, module.name()) {
            Ok(py_module) => py_module,
            Err(err) => {
                diagnostics.extend(vec![Diagnostic::from_py_err(
                    py,
                    &err,
                    DiagnosticScope::Setup,
                    &module.path().to_string(),
                )]);
                return;
            }
        };

        for function in module.test_cases() {
            let mut current_function_fixtures: CalledFixtures<'a> = HashMap::new();

            let function_module_fixtures =
                module.called_fixtures(py, &[FixtureScope::Function], &[function]);
            for (name, fixture) in function_module_fixtures {
                current_function_fixtures.insert(name, fixture);
            }

            let function_package_fixtures =
                package.called_fixtures(py, &[FixtureScope::Function], &[function]);
            for (name, fixture) in function_package_fixtures {
                current_function_fixtures.insert(name, fixture);
            }

            for parent in parents {
                let parent_fixtures =
                    parent.called_fixtures(py, &[FixtureScope::Function], &[function]);
                for (name, fixture) in parent_fixtures {
                    current_function_fixtures.insert(name, fixture);
                }
            }

            for (name, fixture) in &current_module_fixtures {
                current_function_fixtures.insert(name.clone(), fixture.clone());
            }

            let test_name = function.to_string();
            tracing::info!("Running test: {}", test_name);

            let test_case_fixtures = TestCaseFixtures::new(&current_function_fixtures);
            if let Some(result) = function.run_test(py, &py_module, &test_case_fixtures) {
                diagnostics.push(result);
            }
        }

        reporter.report();
    }

    fn test_package<'a>(
        &self,
        py: Python<'a>,
        package: &'a Package<'a>,
        parents: &'a [&'a Package<'a>],
        parent_fixtures: &'a CalledFixtures<'a>,
        diagnostics: &mut Vec<Diagnostic>,
        reporter: &dyn Reporter,
    ) {
        if package.total_test_cases() == 0 {
            return;
        }

        let mut current_package_fixtures: CalledFixtures<'a> = HashMap::new();

        let package_fixtures = package.called_fixtures(
            py,
            &[FixtureScope::Package, FixtureScope::Session],
            &package.direct_test_cases(),
        );

        for (name, fixture) in package_fixtures {
            current_package_fixtures.insert(name, fixture);
        }

        for parent in parents {
            let parent_fixtures =
                parent.called_fixtures(py, &[FixtureScope::Package], &package.direct_test_cases());

            for (name, fixture) in parent_fixtures {
                current_package_fixtures.insert(name, fixture);
            }
        }

        for (name, fixture) in parent_fixtures {
            current_package_fixtures.insert(name.clone(), fixture.clone());
        }

        let mut new_parents = Vec::new();
        new_parents.extend_from_slice(parents);
        new_parents.push(package);

        for module in package.modules().values() {
            self.test_module(
                py,
                module,
                package,
                parents,
                &current_package_fixtures,
                diagnostics,
                reporter,
            );
        }

        for sub_package in package.packages().values() {
            self.test_package(
                py,
                sub_package,
                &new_parents,
                &current_package_fixtures,
                diagnostics,
                reporter,
            );
        }
    }
}

impl TestRunner for StandardTestRunner<'_> {
    fn test(&self) -> RunDiagnostics {
        self.test_impl(&mut DummyReporter)
    }

    fn test_with_reporter(&self, reporter: &mut dyn Reporter) -> RunDiagnostics {
        self.test_impl(reporter)
    }
}

impl TestRunner for Project {
    fn test(&self) -> RunDiagnostics {
        let test_runner = StandardTestRunner::new(self);
        test_runner.test()
    }

    fn test_with_reporter(&self, reporter: &mut dyn Reporter) -> RunDiagnostics {
        let test_runner = StandardTestRunner::new(self);
        test_runner.test_with_reporter(reporter)
    }
}
