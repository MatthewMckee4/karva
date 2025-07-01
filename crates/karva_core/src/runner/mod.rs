use karva_project::project::Project;
use pyo3::prelude::*;

use crate::{
    diagnostic::{
        Diagnostic, DiagnosticScope, ErrorType, Severity,
        reporter::{DummyReporter, Reporter},
    },
    discovery::Discoverer,
    fixture::{FixtureManager, FixtureScope, RequiresFixtures},
    models::{Module, Package},
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

            diagnostics.add_diagnostics(fixture_manager.reset_session_fixtures(py));
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
                    Some(module.path().to_string()),
                    Severity::Error(ErrorType::Unknown),
                ));
                return diagnostics;
            }
        };

        for function in module.test_cases() {
            let mut get_function_fixture_manager =
                |f: &mut dyn FnMut(&mut FixtureManager) -> RunDiagnostics| {
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

            let result = function.test(py, module, &py_module, &mut get_function_fixture_manager);

            diagnostics.update(&result);
        }

        diagnostics.add_diagnostics(fixture_manager.reset_module_fixtures(py));

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
        diagnostics.add_diagnostics(fixture_manager.reset_package_fixtures(py));
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
    use karva_project::tests::TestEnv;

    use super::*;

    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies_different_scopes_with_fixture_in_function()
     {
        let env = TestEnv::new();

        let tests_dir = env.create_tests_dir();
        let inner_dir = tests_dir.join("inner");

        env.create_file(
            tests_dir.join("conftest.py").as_std_path(),
            r"
import karva
@karva.fixture(scope='function')
def x():
    return 1

@karva.fixture(scope='function')
def y(x):
    return 1

@karva.fixture(scope='function')
def z(x, y):
    return 1
",
        );
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

        let tests_dir = env.create_tests_dir();
        env.create_file(
            tests_dir.join("conftest.py").as_std_path(),
            r"
import karva
@karva.fixture(scope='module')
def x():
    return 1
",
        );
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

    #[test]
    fn test_parametrize_two_decorators() {
        let env = TestEnv::new();

        let test_dir = env.create_tests_dir();
        env.create_file(
            test_dir.join("test_parametrize_fixture.py").as_ref(),
            r#"import karva

@karva.tags.parametrize("a", [1, 2])
@karva.tags.parametrize("b", [1, 2])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
"#,
        );

        let project = Project::new(env.cwd(), vec![test_dir]);

        let result = project.test_with_reporter(&mut DummyReporter);

        let mut expected_stats = DiagnosticStats::default();
        expected_stats.add_passed();
        expected_stats.add_passed();
        expected_stats.add_passed();
        expected_stats.add_passed();
        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_parametrize_three_decorators() {
        let env = TestEnv::new();

        let test_dir = env.create_tests_dir();
        env.create_file(
            test_dir.join("test_parametrize_fixture.py").as_ref(),
            r#"import karva

@karva.tags.parametrize("a", [1, 2])
@karva.tags.parametrize("b", [1, 2])
@karva.tags.parametrize("c", [1, 2])
def test_function(a: int, b: int, c: int):
    assert a > 0 and b > 0 and c > 0
"#,
        );

        let project = Project::new(env.cwd(), vec![test_dir]);

        let result = project.test_with_reporter(&mut DummyReporter);

        let mut expected_stats = DiagnosticStats::default();
        for _ in 0..8 {
            expected_stats.add_passed();
        }
        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_fixture_generator() {
        let env = TestEnv::new();

        let test_dir = env.create_tests_dir();
        env.create_file(
            test_dir.join("test_fixture_generator.py").as_ref(),
            r"import karva

@karva.fixture
def fixture_generator():
    yield 1

def test_fixture_generator(fixture_generator):
    assert fixture_generator == 1
",
        );

        let project = Project::new(env.cwd(), vec![test_dir]);

        let result = project.test_with_reporter(&mut DummyReporter);

        let mut expected_stats = DiagnosticStats::default();
        expected_stats.add_passed();
        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_fixture_generator_two_yields() {
        let env = TestEnv::new();

        let test_dir = env.create_tests_dir();
        env.create_file(
            test_dir.join("test_fixture_generator.py").as_ref(),
            r"import karva

@karva.fixture
def fixture_generator():
    yield 1
    yield 2

def test_fixture_generator(fixture_generator):
    assert fixture_generator == 1
",
        );

        let project = Project::new(env.cwd(), vec![test_dir]);

        let result = project.test_with_reporter(&mut DummyReporter);

        let mut expected_stats = DiagnosticStats::default();
        expected_stats.add_passed();
        assert_eq!(*result.stats(), expected_stats);
        assert_eq!(result.diagnostics.len(), 1);
        let first_diagnostic = &result.diagnostics[0];
        let expected_diagnostic = Diagnostic::warning(
            "fixture-error",
            "Fixture fixture_generator had more than one yield statement".to_string(),
            None,
            DiagnosticScope::Teardown,
        );
        assert_eq!(*first_diagnostic, expected_diagnostic);
    }

    #[test]
    fn test_fixture_generator_fail_in_teardown() {
        let env = TestEnv::new();

        let test_dir = env.create_tests_dir();
        env.create_file(
            test_dir.join("test_fixture_generator.py").as_ref(),
            r#"import karva

@karva.fixture
def fixture_generator():
    yield 1
    raise ValueError("fixture-error")

def test_fixture_generator(fixture_generator):
    assert fixture_generator == 1
"#,
        );

        let project = Project::new(env.cwd(), vec![test_dir]);

        let result = project.test_with_reporter(&mut DummyReporter);

        let mut expected_stats = DiagnosticStats::default();
        expected_stats.add_passed();
        assert_eq!(*result.stats(), expected_stats);
        assert_eq!(result.diagnostics.len(), 1);
        let first_diagnostic = &result.diagnostics[0];
        assert_eq!(*first_diagnostic.scope(), DiagnosticScope::Teardown);
        assert_eq!(first_diagnostic.sub_diagnostics().len(), 1);
        let sub_diagnostic = &first_diagnostic.sub_diagnostics()[0];
        assert_eq!(
            sub_diagnostic.message(),
            "Failed to reset fixture fixture_generator"
        );
        assert_eq!(sub_diagnostic.location(), None);
        assert_eq!(
            *sub_diagnostic.severity(),
            Severity::Warning("fixture-error".to_string())
        );
    }

    #[test]
    fn test_fixture_with_name_parameter() {
        let env = TestEnv::new();

        let test_dir = env.create_tests_dir();
        env.create_file(
            test_dir
                .join("test_fixture_with_name_parameter.py")
                .as_ref(),
            r#"import karva

@karva.fixture(name="fixture_name")
def fixture_1():
    return 1

def test_fixture_with_name_parameter(fixture_name):
    assert fixture_name == 1
"#,
        );

        let project = Project::new(env.cwd(), vec![test_dir]);

        let result = project.test_with_reporter(&mut DummyReporter);

        let mut expected_stats = DiagnosticStats::default();
        expected_stats.add_passed();
        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_fixture_is_different_in_different_functions() {
        let env = TestEnv::new();

        let test_dir = env.create_tests_dir();
        env.create_file(
            test_dir
                .join("test_fixture_is_different_in_different_functions.py")
                .as_ref(),
            r"import karva

class TestEnv:
    def __init__(self):
        self.x = 1

@karva.fixture
def fixture():
    return TestEnv()

def test_fixture(fixture):
    assert fixture.x == 1
    fixture.x = 2

def test_fixture_2(fixture):
    assert fixture.x == 1
    fixture.x = 2
",
        );

        let project = Project::new(env.cwd(), vec![test_dir]);

        let result = project.test_with_reporter(&mut DummyReporter);

        let mut expected_stats = DiagnosticStats::default();
        expected_stats.add_passed();
        expected_stats.add_passed();
        assert_eq!(*result.stats(), expected_stats);
    }
}
