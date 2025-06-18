use karva_project::project::Project;
use pyo3::prelude::*;

use crate::{
    diagnostic::{
        Diagnostic, DiagnosticScope,
        reporter::{DummyReporter, Reporter},
    },
    discovery::Discoverer,
    fixture::{FixtureManager, FixtureScope, UsesFixture},
    module::Module,
    package::Package,
    utils::{Upcast, add_to_sys_path, set_output},
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
            let _ = set_output(py, self.project.options.show_output);

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

            let upcast_test_cases: Vec<&dyn UsesFixture> = session.test_cases().upcast();

            fixture_manager.add_fixtures(
                py,
                &[],
                &session,
                &[FixtureScope::Session],
                upcast_test_cases.as_slice(),
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
        let module_test_cases = module.dependencies();
        let upcast_module_test_cases: Vec<&dyn UsesFixture> = module_test_cases.upcast();
        if upcast_module_test_cases.is_empty() {
            return;
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
            let test_cases = [function].to_vec();
            let upcast_test_cases: Vec<&dyn UsesFixture> = test_cases.upcast();

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
        let package_test_cases = package.dependencies();

        let upcast_package_test_cases: Vec<&dyn UsesFixture> = package_test_cases.upcast();

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

#[cfg(test)]
mod tests {
    use karva_project::tests::{MockFixture, TestEnv, mock_fixture};

    use super::*;
    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies_different_scopes_with_fixture_in_function()
     {
        let env = TestEnv::new();

        let fixtures = mock_fixture(&[
            MockFixture {
                name: "x".to_string(),
                scope: "module".to_string(),
                body: "return 1".to_string(),
                args: String::new(),
            },
            MockFixture {
                name: "y".to_string(),
                scope: "function".to_string(),
                body: "return 1".to_string(),
                args: "x".to_string(),
            },
            MockFixture {
                name: "z".to_string(),
                scope: "function".to_string(),
                body: "return 1".to_string(),
                args: "x, y".to_string(),
            },
        ]);
        let tests_dir = env.create_tests_dir();
        let inner_dir = tests_dir.join("inner");

        env.create_file(tests_dir.join("conftest.py").as_std_path(), &fixtures);
        env.create_file(
            inner_dir.join("test_1.py").as_std_path(),
            "def test_1(z): pass",
        );

        let project = Project::new(env.cwd(), vec![tests_dir]);

        let test_runner = StandardTestRunner::new(&project);

        let diagnostics = test_runner.test();

        assert_eq!(diagnostics.diagnostics.len(), 0);
    }
}
