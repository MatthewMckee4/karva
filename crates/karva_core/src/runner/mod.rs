use karva_project::Project;
#[cfg(test)]
use karva_project::testing::TestEnv;

use crate::{
    collection::TestCaseCollector,
    diagnostic::reporter::{DummyReporter, Reporter},
    discovery::StandardDiscoverer,
    utils::with_gil,
};

mod diagnostic;

pub(crate) use diagnostic::RunDiagnostics;

pub trait TestRunner {
    fn test(&self) -> RunDiagnostics {
        self.test_with_reporter(&mut DummyReporter)
    }
    fn test_with_reporter(&self, reporter: &mut dyn Reporter) -> RunDiagnostics;
}

pub(crate) struct StandardTestRunner<'proj> {
    project: &'proj Project,
}

impl<'proj> StandardTestRunner<'proj> {
    #[must_use]
    pub(crate) const fn new(project: &'proj Project) -> Self {
        Self { project }
    }

    fn test_impl(&self, reporter: &mut dyn Reporter) -> RunDiagnostics {
        let (session, discovery_diagnostics) = with_gil(self.project, |py| {
            StandardDiscoverer::new(self.project).discover(py)
        });

        let collected_session =
            with_gil(self.project, |py| TestCaseCollector::collect(py, &session));

        let total_test_cases = collected_session.total_test_cases();

        tracing::info!(
            "Discovered {} test{}",
            total_test_cases,
            if total_test_cases == 1 { "" } else { "s" },
        );

        let mut diagnostics = RunDiagnostics::default();

        diagnostics.add_diagnostics(discovery_diagnostics);

        if total_test_cases == 0 {
            return diagnostics;
        }

        reporter.set(total_test_cases);

        with_gil(self.project, |py| {
            diagnostics.update(&collected_session.run_with_reporter(self.project, py, reporter));
        });

        diagnostics
    }
}

impl TestRunner for StandardTestRunner<'_> {
    fn test_with_reporter(&self, reporter: &mut dyn Reporter) -> RunDiagnostics {
        self.test_impl(reporter)
    }
}

impl TestRunner for Project {
    fn test_with_reporter(&self, reporter: &mut dyn Reporter) -> RunDiagnostics {
        let test_runner = StandardTestRunner::new(self);
        test_runner.test_with_reporter(reporter)
    }
}

#[cfg(test)]
impl TestRunner for TestEnv {
    fn test_with_reporter(&self, reporter: &mut dyn Reporter) -> RunDiagnostics {
        let project = Project::new(self.cwd(), vec![self.cwd()]);
        let test_runner = StandardTestRunner::new(&project);
        test_runner.test_with_reporter(reporter)
    }
}

#[cfg(test)]
mod tests {
    use karva_project::{path::SystemPathBuf, testing::TestEnv};

    use super::*;
    use crate::{
        diagnostic::{Diagnostic, DiagnosticSeverity},
        runner::diagnostic::DiagnosticStats,
    };

    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies_different_scopes_with_fixture_in_function()
     {
        let env = TestEnv::with_files([
            (
                "<test>/conftest.py",
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
            ),
            ("<test>/inner/test_file.py", "def test_1(z): pass"),
        ]);

        let result = env.test();

        assert!(result.passed(), "{result:?}");
    }

    #[test]
    fn test_runner_given_nested_path() {
        let env = TestEnv::with_files([
            (
                "<test>/conftest.py",
                r"
import karva
@karva.fixture(scope='module')
def x():
    return 1
            ",
            ),
            ("<test>/test_file.py", "def test_1(x): pass"),
        ]);

        let result = env.test();

        assert!(result.passed(), "{result:?}");
    }

    #[test]
    fn test_parametrize_with_fixture() {
        let env = TestEnv::with_file(
            "<test>/test_file.py",
            r#"
import karva

@karva.fixture
def fixture_value():
    return 42

@karva.tags.parametrize("a", [1, 2, 3])
def test_parametrize_with_fixture(a, fixture_value):
    assert a > 0
    assert fixture_value == 42"#,
        );

        let result = env.test();

        let mut expected_stats = DiagnosticStats::default();

        for _ in 0..3 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrize_with_fixture_parametrize_priority() {
        let env = TestEnv::with_file(
            "<test>/test_file.py",
            r#"import karva

@karva.fixture
def a():
    return -1

@karva.tags.parametrize("a", [1, 2, 3])
def test_parametrize_with_fixture(a):
    assert a > 0"#,
        );

        let result = env.test();

        let mut expected_stats = DiagnosticStats::default();

        for _ in 0..3 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrize_two_decorators() {
        let env = TestEnv::with_file(
            "<test>/test_file.py",
            r#"import karva

@karva.tags.parametrize("a", [1, 2])
@karva.tags.parametrize("b", [1, 2])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
"#,
        );

        let result = env.test();

        let mut expected_stats = DiagnosticStats::default();

        for _ in 0..4 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_parametrize_three_decorators() {
        let env = TestEnv::with_file(
            "<test>/test_file.py",
            r#"
import karva

@karva.tags.parametrize("a", [1, 2])
@karva.tags.parametrize("b", [1, 2])
@karva.tags.parametrize("c", [1, 2])
def test_function(a: int, b: int, c: int):
    assert a > 0 and b > 0 and c > 0
"#,
        );

        let result = env.test();

        let mut expected_stats = DiagnosticStats::default();

        for _ in 0..8 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_fixture_generator() {
        let env = TestEnv::with_file(
            "<test>/test_file.py",
            r"
import karva

@karva.fixture
def fixture_generator():
    yield 1

def test_fixture_generator(fixture_generator):
    assert fixture_generator == 1
",
        );

        let result = env.test();

        let mut expected_stats = DiagnosticStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_fixture_generator_two_yields() {
        let env = TestEnv::with_file(
            "<test>/test_file.py",
            r"import karva

@karva.fixture
def fixture_generator():
    yield 1
    yield 2

def test_fixture_generator(fixture_generator):
    assert fixture_generator == 1
",
        );

        let result = env.test();

        let mut expected_stats = DiagnosticStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats, "{result:?}");

        assert_eq!(result.diagnostics().len(), 1);
        let first_diagnostic = &result.diagnostics()[0];
        let expected_diagnostic = Diagnostic::warning(
            "fixture-error",
            Some("Fixture fixture_generator had more than one yield statement".to_string()),
            None,
        );

        assert_eq!(*first_diagnostic, expected_diagnostic);
    }

    #[test]
    fn test_fixture_generator_fail_in_teardown() {
        let env = TestEnv::with_file(
            "<test>/test_file.py",
            r#"import karva

@karva.fixture
def fixture_generator():
    yield 1
    raise ValueError("fixture-error")

def test_fixture_generator(fixture_generator):
    assert fixture_generator == 1
"#,
        );

        let result = env.test();

        let mut expected_stats = DiagnosticStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats, "{result:?}");

        assert_eq!(result.diagnostics().len(), 1);
        let first_diagnostic = &result.diagnostics()[0];
        assert_eq!(
            first_diagnostic.inner().message(),
            Some("Failed to reset fixture fixture_generator")
        );
        assert_eq!(
            first_diagnostic.severity(),
            &DiagnosticSeverity::Warning("fixture-error".to_string())
        );
    }

    #[test]
    fn test_fixture_with_name_parameter() {
        let env = TestEnv::with_file(
            "<test>/test_file.py",
            r#"import karva

@karva.fixture(name="fixture_name")
def fixture_1():
    return 1

def test_fixture_with_name_parameter(fixture_name):
    assert fixture_name == 1
"#,
        );

        let result = env.test();

        let mut expected_stats = DiagnosticStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_fixture_is_different_in_different_functions() {
        let env = TestEnv::with_file(
            "<test>/test_file.py",
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

        let result = env.test();

        let mut expected_stats = DiagnosticStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_single_function() {
        let env = TestEnv::with_files([(
            "<test>/test_file.py",
            r"
            def test_1(): pass
            def test_2(): pass",
        )]);

        let mapped_path = env.mapped_path("<test>").unwrap().clone();

        let test_file1_path = mapped_path.join("test_file.py");

        let project = Project::new(
            env.cwd(),
            vec![SystemPathBuf::from(format!(
                "{}::test_1",
                test_file1_path.display()
            ))],
        );

        let test_runner = StandardTestRunner::new(&project);

        let result = test_runner.test();

        let mut expected_stats = DiagnosticStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_single_function_shadowed_by_file() {
        let env = TestEnv::with_files([(
            "<test>/test_file.py",
            r"
            def test_1(): pass
            def test_2(): pass",
        )]);

        let mapped_path = env.mapped_path("<test>").unwrap().clone();

        let test_file1_path = mapped_path.join("test_file.py");

        let project = Project::new(
            env.cwd(),
            vec![
                SystemPathBuf::from(format!("{}::test_1", test_file1_path.display())),
                test_file1_path,
            ],
        );

        let test_runner = StandardTestRunner::new(&project);

        let result = test_runner.test();

        let mut expected_stats = DiagnosticStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_single_function_shadowed_by_directory() {
        let env = TestEnv::with_files([(
            "<test>/test_file.py",
            r"
            def test_1(): pass
            def test_2(): pass",
        )]);

        let mapped_path = env.mapped_path("<test>").unwrap().clone();

        let test_file1_path = mapped_path.join("test_file.py");

        let project = Project::new(
            env.cwd(),
            vec![
                SystemPathBuf::from(format!("{}::test_1", test_file1_path.display())),
                mapped_path,
            ],
        );

        let test_runner = StandardTestRunner::new(&project);

        let result = test_runner.test();

        let mut expected_stats = DiagnosticStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }
}
