use karva_project::project::Project;
use pyo3::prelude::*;

use crate::{
    diagnostic::{
        Diagnostic, DiagnosticScope,
        reporter::{DummyReporter, Reporter},
    },
    discovery::Discoverer,
    fixture::{FixtureManager, FixtureScope},
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

            let mut fixture_manager = FixtureManager::new();

            fixture_manager.add_session_fixtures(
                py,
                &session,
                &[FixtureScope::Session],
                &session.test_cases(),
            );

            self.test_package(
                py,
                &session,
                &[],
                &mut fixture_manager,
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
        parents: &[&'a Package<'a>],
        fixture_manager: &mut FixtureManager<'a>,
        diagnostics: &mut Vec<Diagnostic>,
        reporter: &dyn Reporter,
    ) {
        if module.total_test_cases() == 0 {
            return;
        }

        fixture_manager.add_module_fixtures(
            py,
            module,
            &[
                FixtureScope::Module,
                FixtureScope::Package,
                FixtureScope::Session,
            ],
            &module.test_cases(),
        );

        for parent in parents {
            fixture_manager.add_module_fixtures(
                py,
                *parent,
                &[FixtureScope::Module],
                &module.test_cases(),
            );
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
            fixture_manager.add_function_fixtures(
                py,
                module,
                &[FixtureScope::Function],
                &[function],
            );

            for parent in parents {
                fixture_manager.add_function_fixtures(
                    py,
                    *parent,
                    &[FixtureScope::Function],
                    &[function],
                );
            }

            let test_name = function.to_string();
            tracing::info!("Running test: {}", test_name);

            if let Some(result) = function.run_test(py, &py_module, fixture_manager) {
                diagnostics.push(result);
                tracing::info!("Test {} failed", test_name);
            } else {
                tracing::info!("Test {} passed", test_name);
            }
            fixture_manager.reset_function_fixtures();
        }

        fixture_manager.reset_module_fixtures();

        reporter.report();
    }

    fn test_package<'a>(
        &self,
        py: Python<'a>,
        package: &'a Package<'a>,
        parents: &[&'a Package<'a>],
        fixture_manager: &mut FixtureManager<'a>,
        diagnostics: &mut Vec<Diagnostic>,
        reporter: &dyn Reporter,
    ) {
        if package.total_test_cases() == 0 {
            return;
        }

        fixture_manager.add_package_fixtures(
            py,
            package,
            &[FixtureScope::Package, FixtureScope::Session],
            &package.direct_test_cases(),
        );

        for parent in parents {
            fixture_manager.add_package_fixtures(
                py,
                *parent,
                &[FixtureScope::Package],
                &package.direct_test_cases(),
            );
        }

        let mut new_parents: Vec<&'a Package<'a>> = parents.to_vec();
        new_parents.push(package);

        for module in package.modules().values() {
            self.test_module(
                py,
                module,
                &new_parents,
                fixture_manager,
                diagnostics,
                reporter,
            );
        }

        for sub_package in package.packages().values() {
            self.test_package(
                py,
                sub_package,
                &new_parents,
                fixture_manager,
                diagnostics,
                reporter,
            );
        }
        fixture_manager.reset_package_fixtures();
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
