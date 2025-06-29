use karva_project::project::Project;
use pyo3::prelude::*;

use crate::{
    diagnostic::{
        Diagnostic, DiagnosticScope,
        reporter::{DummyReporter, Reporter},
    },
    discovery::Discoverer,
    fixture::{FixtureManager, FixtureScope, RequiresFixtures},
    module::Module,
    package::Package,
    utils::{Upcast, with_gil},
};

mod diagnostic;

pub use diagnostic::{DiagnosticStats, RunDiagnostics};

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
            "Discovered {} test{} in {} file{}",
            total_test_cases,
            if total_test_cases == 1 { "" } else { "s" },
            total_files,
            if total_files == 1 { "" } else { "s" }
        );

        reporter.set(total_files);

        let mut diagnostics = RunDiagnostics::default();

        diagnostics.add_diagnostics(discovery_diagnostics);
        with_gil(self.project, |py| {
            let mut fixture_manager = FixtureManager::new();

            let upcast_test_cases: Vec<&dyn RequiresFixtures> = session.test_cases().upcast();

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

        diagnostics
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::unused_self)]
    fn test_module<'a>(
        &self,
        py: Python<'a>,
        module: &'a Module<'a>,
        parents: &[&'a Package<'a>],
        fixture_manager: &mut FixtureManager,
        reporter: &dyn Reporter,
    ) -> RunDiagnostics {
        let mut diagnostics = RunDiagnostics::default();
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
                    &module.path().to_string(),
                ));
                return diagnostics;
            }
        };

        for function in module.test_cases() {
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

            let result = function.test(py, &py_module, fixture_manager);

            diagnostics.update(&result);

            fixture_manager.reset_function_fixtures();
        }

        fixture_manager.reset_module_fixtures();

        reporter.report();

        diagnostics
    }

    fn test_package<'a>(
        &self,
        py: Python<'a>,
        package: &'a Package<'a>,
        parents: &[&'a Package<'a>],
        fixture_manager: &mut FixtureManager,
        diagnostics: &mut RunDiagnostics,
        reporter: &dyn Reporter,
    ) {
        if package.total_test_cases() == 0 {
            return;
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
            diagnostics.update(&module_diagnostics);
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

    #[test]
    fn test_runner_given_nested_path() {
        let env = TestEnv::new();

        let fixtures = mock_fixture(&[MockFixture {
            name: "x".to_string(),
            scope: "module".to_string(),
            body: "return 1".to_string(),
            args: String::new(),
        }]);
        let tests_dir = env.create_tests_dir();
        env.create_file(tests_dir.join("conftest.py").as_std_path(), &fixtures);
        let test_file = env.create_file(
            tests_dir.join("test_1.py").as_std_path(),
            "def test_1(x): pass",
        );

        let project = Project::new(env.cwd(), vec![test_file]);

        let test_runner = StandardTestRunner::new(&project);

        let diagnostics = test_runner.test();

        assert_eq!(diagnostics.diagnostics.len(), 0);
    }

    #[test]
    fn test_parametrize_with_fixture() {
        let env = TestEnv::new();
        let test_dir = env.create_tests_dir();
        env.create_file(
            test_dir.join("test_parametrize_fixture.py").as_ref(),
            r#"import karva

@karva.fixture
def fixture_value():
    return 42

@karva.tags.parametrize("a", [1, 2, 3])
def test_parametrize_with_fixture(a, fixture_value):
    assert a > 0
    assert fixture_value == 42"#,
        );

        let project = Project::new(env.cwd(), vec![test_dir]);

        let result = project.test_with_reporter(&mut DummyReporter);

        let mut expected_stats = DiagnosticStats::default();
        expected_stats.add_passed();
        expected_stats.add_passed();
        expected_stats.add_passed();
        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_parametrize_with_fixture_parametrize_priority() {
        let env = TestEnv::new();

        let test_dir = env.create_tests_dir();
        env.create_file(
            test_dir.join("test_parametrize_fixture.py").as_ref(),
            r#"import karva

@karva.fixture
def a():
    return -1

@karva.tags.parametrize("a", [1, 2, 3])
def test_parametrize_with_fixture(a):
    assert a > 0"#,
        );

        let project = Project::new(env.cwd(), vec![test_dir]);

        let result = project.test_with_reporter(&mut DummyReporter);

        let mut expected_stats = DiagnosticStats::default();
        expected_stats.add_passed();
        expected_stats.add_passed();
        expected_stats.add_passed();
        assert_eq!(*result.stats(), expected_stats);
    }
}
