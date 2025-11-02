use karva_project::Project;
#[cfg(test)]
use karva_test::TestContext;

use crate::{
    collection::TestCaseCollector,
    diagnostic::reporter::{DummyReporter, Reporter},
    discovery::StandardDiscoverer,
    utils::attach,
};

pub(crate) mod diagnostic;

pub(crate) use diagnostic::TestRunResult;

pub trait TestRunner {
    fn test(&self) -> TestRunResult {
        self.test_with_reporter(&DummyReporter)
    }
    fn test_with_reporter(&self, reporter: &dyn Reporter) -> TestRunResult;
}

pub(crate) struct StandardTestRunner<'proj> {
    project: &'proj Project,
}

impl<'proj> StandardTestRunner<'proj> {
    pub(crate) const fn new(project: &'proj Project) -> Self {
        Self { project }
    }

    fn test_impl(&self, reporter: &dyn Reporter) -> TestRunResult {
        attach(self.project, |py| {
            let mut diagnostics = TestRunResult::default();

            let (session, discovery_diagnostics) =
                StandardDiscoverer::new(self.project).discover(py);

            let collected_session = TestCaseCollector::collect(py, &session);

            let total_test_cases = collected_session.total_test_cases();

            tracing::info!(
                "Collected {} test{}",
                total_test_cases,
                if total_test_cases == 1 { "" } else { "s" },
            );

            diagnostics.add_diagnostics(discovery_diagnostics);

            diagnostics.update(&collected_session.run_with_reporter(py, reporter));

            diagnostics
        })
    }
}

impl TestRunner for StandardTestRunner<'_> {
    fn test_with_reporter(&self, reporter: &dyn Reporter) -> TestRunResult {
        self.test_impl(reporter)
    }
}

impl TestRunner for Project {
    fn test_with_reporter(&self, reporter: &dyn Reporter) -> TestRunResult {
        let test_runner = StandardTestRunner::new(self);
        test_runner.test_with_reporter(reporter)
    }
}

#[cfg(test)]
mod tests {
    use camino::Utf8PathBuf;
    use karva_project::utils::module_name;
    use rstest::rstest;

    use super::*;
    use crate::{
        diagnostic::{Diagnostic, DiagnosticSeverity},
        runner::diagnostic::TestResultStats,
    };

    impl TestRunner for TestContext {
        fn test_with_reporter(&self, reporter: &dyn Reporter) -> TestRunResult {
            let project = Project::new(self.cwd(), vec![self.cwd()]);
            let test_runner = StandardTestRunner::new(&project);
            test_runner.test_with_reporter(reporter)
        }
    }
    fn get_auto_use_kw(framework: &str) -> &str {
        match framework {
            "pytest" => "autouse",
            "karva" => "auto_use",
            _ => panic!("Invalid framework"),
        }
    }

    fn get_skip_function(framework: &str) -> &str {
        match framework {
            "pytest" => "pytest.mark.skip",
            "karva" => "karva.tags.skip",
            _ => panic!("Invalid framework"),
        }
    }

    fn get_parametrize_function(framework: &str) -> &str {
        match framework {
            "pytest" => "pytest.mark.parametrize",
            "karva" => "karva.tags.parametrize",
            _ => panic!("Invalid framework"),
        }
    }

    #[test]
    fn test_single_file() {
        let test_context = TestContext::with_files([
            (
                "<test>/test_file1.py",
                r"
                def test_1(): pass
                def test_2(): pass",
            ),
            (
                "<test>/test_file2.py",
                r"
                def test_3(): pass
                def test_4(): pass",
            ),
        ]);

        let mapped_path = test_context.mapped_path("<test>").unwrap().clone();
        let test_file1_path = mapped_path.join("test_file1.py");

        let project = Project::new(test_context.cwd(), vec![test_file1_path]);

        let test_runner = StandardTestRunner::new(&project);

        let result = test_runner.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_empty_file() {
        let test_context = TestContext::with_file("<test>/test_empty.py", "");

        let result = test_context.test();

        let expected_stats = TestResultStats::default();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_empty_directory() {
        let test_context = TestContext::with_file("<test>/tests/test_empty.py", "");

        let mapped_tests_dir = test_context.mapped_path("<test>").unwrap();

        let project = Project::new(test_context.cwd(), vec![mapped_tests_dir.clone()]);

        let test_runner = StandardTestRunner::new(&project);

        let result = test_runner.test();

        let expected_stats = TestResultStats::default();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_fixture_manager_add_fixtures_impl_three_dependencies_different_scopes_with_fixture_in_function()
     {
        let test_context = TestContext::with_files([
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

        let result = test_context.test();

        assert!(result.passed(), "{result:?}");
    }

    #[test]
    fn test_runner_given_nested_path() {
        let test_context = TestContext::with_files([
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

        let result = test_context.test();

        assert!(result.passed(), "{result:?}");
    }

    #[test]
    fn test_parametrize_with_fixture() {
        let test_context = TestContext::with_file(
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

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..3 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrize_with_fixture_parametrize_priority() {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            r#"import karva

@karva.fixture
def a():
    return -1

@karva.tags.parametrize("a", [1, 2, 3])
def test_parametrize_with_fixture(a):
    assert a > 0"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..3 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrize_two_decorators() {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            r#"import karva

@karva.tags.parametrize("a", [1, 2])
@karva.tags.parametrize("b", [1, 2])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..4 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_parametrize_three_decorators() {
        let test_context = TestContext::with_file(
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

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..8 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_fixture_generator() {
        let test_context = TestContext::with_file(
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

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[rstest]
    fn test_fixture_generator_with_second_fixture(#[values("karva", "pytest")] framework: &str) {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            &format!(
                r"
import {framework}

@{framework}.fixture
def first_fixture():
    pass

@{framework}.fixture
def fixture_generator(first_fixture):
    yield 1

def test_fixture_generator(fixture_generator):
    assert fixture_generator == 1
"
            ),
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_fixture_generator_two_yields() {
        let test_context = TestContext::with_file(
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

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        let module_name_path = test_context
            .mapped_path("<test>")
            .unwrap()
            .join("test_file.py");
        let module_name = module_name(&test_context.cwd(), &module_name_path).unwrap();

        assert_eq!(*result.stats(), expected_stats, "{result:?}");

        assert_eq!(result.diagnostics().len(), 1);
        let first_diagnostic = &result.diagnostics()[0];
        let expected_diagnostic = Diagnostic::warning(
            "fixture-error",
            Some(format!(
                "Fixture {module_name}::fixture_generator had more than one yield statement"
            )),
            None,
        );

        assert_eq!(*first_diagnostic, expected_diagnostic);
    }

    #[test]
    fn test_fixture_generator_fail_in_teardown() {
        let test_context = TestContext::with_file(
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

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        let module_name_path = test_context
            .mapped_path("<test>")
            .unwrap()
            .join("test_file.py");
        let module_name = module_name(&test_context.cwd(), &module_name_path).unwrap();

        assert_eq!(*result.stats(), expected_stats, "{result:?}");

        assert_eq!(result.diagnostics().len(), 1);
        let first_diagnostic = &result.diagnostics()[0];
        assert_eq!(
            first_diagnostic.inner().message(),
            Some(format!("Failed to reset fixture {module_name}::fixture_generator").as_str()),
        );
        assert_eq!(
            first_diagnostic.severity(),
            &DiagnosticSeverity::Warning("fixture-error".to_string())
        );
    }

    #[test]
    fn test_fixture_with_name_parameter() {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            r#"import karva

@karva.fixture(name="fixture_name")
def fixture_1():
    return 1

def test_fixture_with_name_parameter(fixture_name):
    assert fixture_name == 1
"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_fixture_is_different_in_different_functions() {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            r"import karva

class Testtest_context:
    def __init__(self):
        self.x = 1

@karva.fixture
def fixture():
    return Testtest_context()

def test_fixture(fixture):
    assert fixture.x == 1
    fixture.x = 2

def test_fixture_2(fixture):
    assert fixture.x == 1
    fixture.x = 2
",
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_single_function() {
        let test_context = TestContext::with_files([(
            "<test>/test_file.py",
            r"
            def test_1(): pass
            def test_2(): pass",
        )]);

        let mapped_path = test_context.mapped_path("<test>").unwrap().clone();

        let test_file1_path = mapped_path.join("test_file.py");

        let project = Project::new(
            test_context.cwd(),
            vec![Utf8PathBuf::from(format!("{test_file1_path}::test_1"))],
        );

        let test_runner = StandardTestRunner::new(&project);

        let result = test_runner.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_single_function_shadowed_by_file() {
        let test_context = TestContext::with_files([(
            "<test>/test_file.py",
            r"
            def test_1(): pass
            def test_2(): pass",
        )]);

        let mapped_path = test_context.mapped_path("<test>").unwrap().clone();

        let test_file1_path = mapped_path.join("test_file.py");

        let project = Project::new(
            test_context.cwd(),
            vec![
                Utf8PathBuf::from(format!("{test_file1_path}::test_1")),
                test_file1_path,
            ],
        );

        let test_runner = StandardTestRunner::new(&project);

        let result = test_runner.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_single_function_shadowed_by_directory() {
        let test_context = TestContext::with_files([(
            "<test>/test_file.py",
            r"
            def test_1(): pass
            def test_2(): pass",
        )]);

        let mapped_path = test_context.mapped_path("<test>").unwrap().clone();

        let test_file1_path = mapped_path.join("test_file.py");

        let project = Project::new(
            test_context.cwd(),
            vec![
                Utf8PathBuf::from(format!("{test_file1_path}::test_1")),
                mapped_path,
            ],
        );

        let test_runner = StandardTestRunner::new(&project);

        let result = test_runner.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_fixture_from_current_package_session_scope() {
        let test_context = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture(scope='session')
def x():
    return 1
            ",
            ),
            ("<test>/tests/test_file.py", "def test_1(x): pass"),
        ]);

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_fixture_from_current_package_function_scope() {
        let test_context = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva
@karva.fixture
def x():
    return 1
            ",
            ),
            ("<test>/tests/test_file.py", "def test_1(x): pass"),
        ]);

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_finalizer_from_current_package_session_scope() {
        let test_context = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva

arr = []

@karva.fixture(scope='session')
def x():
    yield 1
    arr.append(1)
            ",
            ),
            (
                "<test>/tests/test_file.py",
                r"
from .conftest import arr

def test_1(x):
    assert len(arr) == 0

def test_2(x):
    assert len(arr) == 0
",
            ),
        ]);

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();
        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_finalizer_from_current_package_function_scope() {
        let test_context = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva

arr = []

@karva.fixture
def x():
    yield 1
    arr.append(1)
            ",
            ),
            (
                "<test>/tests/test_file.py",
                r"
from .conftest import arr

def test_1(x):
    assert len(arr) == 0

def test_2(x):
    assert len(arr) == 1
",
            ),
        ]);

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();
        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_discover_pytest_fixture() {
        let test_context = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import pytest

@pytest.fixture
def x():
    return 1
",
            ),
            ("<test>/tests/test_1.py", "def test_1(x): pass"),
        ]);

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[rstest]
    fn test_dynamic_fixture_scope_session_scope(#[values("pytest", "karva")] framework: &str) {
        let test_context = TestContext::with_file(
            "<test>/test_dynamic_scope.py",
            &format!(
                r#"
from {framework} import fixture

def dynamic_scope(fixture_name, config):
    if fixture_name.endswith("_session"):
        return "session"
    return "function"

@fixture(scope=dynamic_scope)
def x_session():
    return []

def test_1(x_session):
    x_session.append(1)
    assert x_session == [1]

def test_2(x_session):
    x_session.append(2)
    assert x_session == [1, 2]
    "#,
            ),
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[rstest]
    fn test_dynamic_fixture_scope_function_scope(#[values("pytest", "karva")] framework: &str) {
        let test_context = TestContext::with_file(
            "<test>/test_dynamic_scope.py",
            &format!(
                r#"
from {framework} import fixture

def dynamic_scope(fixture_name, config):
    if fixture_name.endswith("_function"):
        return "function"
    return "function"

@fixture(scope=dynamic_scope)
def x_function():
    return []

def test_1(x_function):
    x_function.append(1)
    assert x_function == [1]

def test_2(x_function):
    x_function.append(2)
    assert x_function == [2]
    "#,
            ),
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_use_fixtures_single_fixture() {
        let test_context = TestContext::with_file(
            "<test>/test_use_fixtures.py",
            r#"
import karva

arr = []

@karva.fixture
def setup_fixture():
    arr.append(1)

@karva.tags.use_fixtures("setup_fixture")
def test_with_use_fixture():
    assert arr == [1]
"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_use_fixtures_multiple_fixtures() {
        let test_context = TestContext::with_file(
            "<test>/test_use_fixtures.py",
            r#"
import karva

arr = []

@karva.fixture
def fixture1():
    arr.append(1)

@karva.fixture
def fixture2():
    arr.append(2)

@karva.tags.use_fixtures("fixture1", "fixture2")
def test_with_multiple_use_fixtures():
    assert arr == [1, 2]
"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();
        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_use_fixtures_combined_with_parameter_fixtures() {
        let test_context = TestContext::with_file(
            "<test>/test_use_fixtures.py",
            r#"
import karva

@karva.fixture
def setup_fixture():
    return "setup_value"

@karva.fixture
def param_fixture():
    return "param_value"

@karva.tags.use_fixtures("setup_fixture")
def test_combined_fixtures(param_fixture):
    # Both setup_fixture (from use_fixtures) and param_fixture (from parameters) should be resolved
    assert True
"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();
        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_use_fixtures_with_parametrize() {
        let test_context = TestContext::with_file(
            "<test>/test_use_fixtures.py",
            r#"
import karva

arr = []

@karva.fixture
def setup_fixture():
    arr.append(1)

@karva.tags.use_fixtures("setup_fixture")
@karva.tags.parametrize("value", [1, 2, 3])
def test_use_fixtures_with_parametrize(value):
    assert value > 0
    # Fixtures are called before any run
    assert arr == [1, 1, 1]
"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();
        for _ in 0..3 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_use_fixtures_multiple_decorators() {
        let test_context = TestContext::with_file(
            "<test>/test_use_fixtures.py",
            r#"
import karva

arr = []

@karva.fixture
def fixture1():
    arr.append(1)

@karva.fixture
def fixture2():
    arr.append(2)

@karva.tags.use_fixtures("fixture1")
@karva.tags.use_fixtures("fixture2")
def test_multiple_use_fixtures_decorators():
    assert arr == [1, 2]
"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();
        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_use_fixtures_fixture_not_found_but_not_used() {
        let test_context = TestContext::with_file(
            "<test>/test_use_fixtures.py",
            r#"
import karva

@karva.tags.use_fixtures("nonexistent_fixture")
def test_missing_fixture():
    assert True
"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_use_fixtures_generator_fixture() {
        let test_context = TestContext::with_file(
            "<test>/test_use_fixtures.py",
            r#"
import karva

arr = []

@karva.fixture
def generator_fixture():
    arr.append(1)
    yield 1

@karva.tags.use_fixtures("generator_fixture")
def test_use_fixtures_with_generator():
    assert arr == [1]
"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();
        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_use_fixtures_session_scope() {
        let test_context = TestContext::with_files([(
            "<test>/test_use_fixtures.py",
            r#"
import karva

arr = []

@karva.fixture(scope='session')
def session_fixture():
    arr.append(1)

@karva.tags.use_fixtures("session_fixture")
def test_session_1():
    assert arr == [1]

@karva.tags.use_fixtures("session_fixture")
def test_session_2():
    assert arr == [1]
"#,
        )]);

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();
        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_use_fixtures_mixed_with_normal_fixtures() {
        let test_context = TestContext::with_files([
            (
                "<test>/conftest.py",
                r#"
import karva

@karva.fixture
def shared_fixture():
    return "shared_value"

@karva.fixture
def use_fixture_only():
    return "use_only_value"
"#,
            ),
            (
                "<test>/test_use_fixtures.py",
                r#"
import karva

@karva.tags.use_fixtures("use_fixture_only")
def test_mixed_fixtures(shared_fixture):
    assert shared_fixture == "shared_value"
"#,
            ),
        ]);

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_pytest_mark_usefixtures_single_fixture() {
        let test_context = TestContext::with_file(
            "<test>/test_pytest_use_fixtures.py",
            r#"
import pytest

arr = []

@pytest.fixture
def setup_fixture():
    arr.append(1)

@pytest.mark.usefixtures("setup_fixture")
def test_with_pytest_use_fixture():
    assert arr == [1]
"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_pytest_mark_usefixtures_multiple_fixtures() {
        let test_context = TestContext::with_file(
            "<test>/test_pytest_use_fixtures.py",
            r#"
import pytest

arr = []

@pytest.fixture
def fixture1():
    arr.append(1)

@pytest.fixture
def fixture2():
    arr.append(2)

@pytest.mark.usefixtures("fixture1", "fixture2")
def test_with_multiple_pytest_use_fixtures():
    assert arr == [1, 2]
"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();
        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_pytest_mark_usefixtures_with_parametrize() {
        let test_context = TestContext::with_file(
            "<test>/test_pytest_use_fixtures.py",
            r#"
import pytest

arr = []

@pytest.fixture
def setup_fixture():
    arr.append(1)

@pytest.mark.usefixtures("setup_fixture")
@pytest.mark.parametrize("value", [1, 2, 3])
def test_pytest_use_fixtures_with_parametrize(value):
    assert value > 0
    # Fixtures are called before any run
    assert arr == [1, 1, 1]
"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();
        for _ in 0..3 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_pytest_mark_usefixtures_session_scope() {
        let test_context = TestContext::with_files([(
            "<test>/test_pytest_use_fixtures.py",
            r#"
import pytest

arr = []

@pytest.fixture(scope='session')
def session_fixture():
    arr.append(1)

@pytest.mark.usefixtures("session_fixture")
def test_pytest_session_1():
    assert arr == [1]

@pytest.mark.usefixtures("session_fixture")
def test_pytest_session_2():
    assert arr == [1]
"#,
        )]);

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();
        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_fixture_override_in_test_modules() {
        let test_context = TestContext::with_files([
            (
                "<test>/tests/conftest.py",
                r"
import karva

@karva.fixture
def username():
    return 'username'
",
            ),
            (
                "<test>/tests/test_something.py",
                r"
import karva

@karva.fixture
def username(username):
    return 'overridden-' + username

def test_username(username):
    assert username == 'overridden-username'
",
            ),
            (
                "<test>/tests/test_something_else.py",
                r"
import karva

@karva.fixture
def username(username):
    return 'overridden-else-' + username

def test_username(username):
    assert username == 'overridden-else-username'
",
            ),
        ]);

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_fixtures_given_by_decorator() {
        let test_context = TestContext::with_file(
            "<test>/test_fixtures_given_by_decorator.py",
            r"
import functools

def given(**kwargs):
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **wrapper_kwargs):
            return func(*args, **kwargs, **wrapper_kwargs)
        return wrapper
    return decorator

@given(a=1)
def test_fixtures_given_by_decorator(a):
    assert a == 1
",
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_fixtures_given_by_decorator_and_fixture() {
        let test_context = TestContext::with_file(
            "<test>/test_fixtures_given_by_decorator.py",
            r"
import karva
import functools

def given(**kwargs):
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **wrapper_kwargs):
            return func(*args, **kwargs, **wrapper_kwargs)
        return wrapper
    return decorator

@karva.fixture
def b():
    return 1

@given(a=1)
def test_fixtures_given_by_decorator(a, b):
    assert a == 1
    assert b == 1
",
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_fixtures_given_by_decorator_and_parametrize() {
        let test_context = TestContext::with_file(
            "<test>/test_fixtures_given_by_decorator.py",
            r#"
import karva
import functools

def given(**kwargs):
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **wrapper_kwargs):
            return func(*args, **kwargs, **wrapper_kwargs)
        return wrapper
    return decorator

@given(a=1)
@karva.tags.parametrize("b", [1, 2])
def test_fixtures_given_by_decorator(a, b):
    assert a == 1
    assert b in [1, 2]
"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_fixtures_given_by_decorator_and_parametrize_and_fixture() {
        let test_context = TestContext::with_file(
            "<test>/test_fixtures_given_by_decorator.py",
            r#"
import karva
import functools

def given(**kwargs):
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **wrapper_kwargs):
            return func(*args, **kwargs, **wrapper_kwargs)
        return wrapper
    return decorator

@karva.fixture
def c():
    return 1

@given(a=1)
@karva.tags.parametrize("b", [1, 2])
def test_fixtures_given_by_decorator(a, b, c):
    assert a == 1
    assert b in [1, 2]
    assert c == 1
"#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_fixtures_given_by_decorator_one_missing() {
        let test_context = TestContext::with_file(
            "<test>/test_fixtures_given_by_decorator.py",
            r"
import functools

def given(**kwargs):
    def decorator(func):
        @functools.wraps(func)
        def wrapper(*args, **wrapper_kwargs):
            return func(*args, **kwargs, **wrapper_kwargs)
        return wrapper
    return decorator

@given(a=1)
def test_fixtures_given_by_decorator(a, b):
    assert a == 1
    assert b == 1
",
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_failed();

        assert!(!result.passed());

        assert_eq!(*result.stats(), expected_stats);
    }

    #[rstest]
    fn test_function_scope_auto_use_fixture(#[values("pytest", "karva")] framework: &str) {
        let test_context = TestContext::with_file(
            "<test>/test_function_scope_auto_use_fixture.py",
            format!(
                r#"
import {framework}

arr = []

@{framework}.fixture(scope="function", {auto_use_kw}=True)
def auto_function_fixture():
    arr.append(1)
    yield
    arr.append(2)

def test_something():
    assert arr == [1, 1]

def test_something_else():
    assert arr == [1, 1, 2]
"#,
                auto_use_kw = get_auto_use_kw(framework),
            )
            .as_str(),
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[rstest]
    fn test_scope_auto_use_fixture(
        #[values("pytest", "karva")] framework: &str,
        #[values("module", "package", "session")] scope: &str,
    ) {
        let test_context = TestContext::with_file(
            "<test>/test_function_scope_auto_use_fixture.py",
            &format!(
                r#"
import {framework}

arr = []

@{framework}.fixture(scope="{scope}", {auto_use_kw}=True)
def auto_function_fixture():
    arr.append(1)
    yield
    arr.append(2)

def test_something():
    assert arr == [1]

def test_something_else():
    assert arr == [1]
"#,
                auto_use_kw = get_auto_use_kw(framework),
            ),
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[rstest]
    fn test_skip(#[values("pytest", "karva")] framework: &str) {
        let test_context = TestContext::with_file(
            "<test>/test_skip.py",
            &format!(
                r"
        import {framework}

        @{decorator}('This test is skipped with decorator')
        def test_1():
            assert False

        ",
                decorator = get_skip_function(framework)
            ),
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_skipped();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[rstest]
    fn test_skip_keyword(#[values("pytest", "karva")] framework: &str) {
        let test_context = TestContext::with_file(
            "<test>/test_skip.py",
            &format!(
                r"
        import {framework}

        @{decorator}(reason='This test is skipped with decorator')
        def test_1():
            assert False

        ",
                decorator = get_skip_function(framework)
            ),
        );
        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_skipped();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[rstest]
    fn test_skip_functionality_no_reason(#[values("pytest", "karva")] framework: &str) {
        let test_context = TestContext::with_file(
            "<test>/test_skip.py",
            &format!(
                r"
        import {framework}

        @{decorator}
        def test_1():
            assert False

        ",
                decorator = get_skip_function(framework)
            ),
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_skipped();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[rstest]
    fn test_skip_reason_function_call(#[values("pytest", "karva")] framework: &str) {
        let test_context = TestContext::with_file(
            "<test>/test_skip.py",
            &format!(
                r"
        import {framework}

        @{decorator}()
        def test_1():
            assert False

        ",
                decorator = get_skip_function(framework)
            ),
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_skipped();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[rstest]
    fn test_nested_generator_fixture(#[values("pytest", "karva")] framework: &str) {
        let test_context = TestContext::with_file(
            "<test>/test_nested_generator_fixture.py",
            &format!(
                r"
                from {framework} import fixture

                class Calculator:
                    def add(self, a: int, b: int) -> int:
                        return a + b

                @fixture
                def calculator() -> Calculator:
                    if 1:
                        yield Calculator()
                    else:
                        yield Calculator()

                def test_calculator(calculator: Calculator) -> None:
                    assert calculator.add(1, 2) == 3
                "
            ),
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[rstest]
    fn test_fixture_order_respects_scope(#[values("pytest", "karva")] framework: &str) {
        let test_context = TestContext::with_file(
            "<test>/test_nested_generator_fixture.py",
            &format!(
                r"
                from {framework} import fixture

                data = {{}}

                @fixture(scope='module')
                def clean_data():
                    data.clear()

                @fixture({auto_use_kw}=True)
                def add_data():
                    data.update(value=True)

                def test_value(clean_data):
                    assert data.get('value')
                ",
                auto_use_kw = get_auto_use_kw(framework)
            ),
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[rstest]
    fn test_auto_use_fixture(#[values("pytest", "karva")] framework: &str) {
        let test_context = TestContext::with_file(
            "<test>/test_nested_generator_fixture.py",
            &format!(
                r#"
                from {framework} import fixture

                @fixture
                def first_entry():
                    return "a"

                @fixture
                def order(first_entry):
                    return []

                @fixture({auto_use_kw}=True)
                def append_first(order, first_entry):
                    return order.append(first_entry)

                def test_string_only(order, first_entry):
                    assert order == [first_entry]

                def test_string_and_int(order, first_entry):
                    order.append(2)
                    assert order == [first_entry, 2]
                "#,
                auto_use_kw = get_auto_use_kw(framework)
            ),
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);
    }

    #[rstest]
    fn test_fixture_initialization_order(#[values("pytest", "karva")] framework: &str) {
        let test_context = TestContext::with_file(
            "<test>/test.py",
            &format!(
                r#"
                    from {framework} import fixture

                    arr = []

                    @fixture(scope="session")
                    def session_fixture() -> int:
                        assert arr == []
                        arr.append(1)
                        return 1

                    @fixture(scope="module")
                    def module_fixture() -> int:
                        assert arr == [1]
                        arr.append(2)
                        return 2

                    @fixture(scope="package")
                    def package_fixture() -> int:
                        assert arr == [1, 2]
                        arr.append(3)
                        return 3

                    @fixture
                    def function_fixture() -> int:
                        assert arr == [1, 2, 3]
                        arr.append(4)
                        return 4

                    def test_all_scopes(
                        session_fixture: int,
                        module_fixture: int,
                        package_fixture: int,
                        function_fixture: int,
                    ) -> None:
                        assert session_fixture == 1
                        assert module_fixture == 2
                        assert package_fixture == 3
                        assert function_fixture == 4
                    "#,
            ),
        );
        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);
    }

    #[test]
    fn test_invalid_pytest_fixture_scope() {
        let test_context = TestContext::with_file(
            "<test>/test.py",
            r#"
                import pytest

                @pytest.fixture(scope="sessionss")
                def some_fixture() -> int:
                    return 1

                def test_all_scopes(
                    some_fixture: int,
                ) -> None:
                    assert some_fixture == 1
                "#,
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_failed();

        assert_eq!(*result.stats(), expected_stats);

        assert!(result.diagnostics().len() == 2);
    }

    #[test]
    fn test_missing_fixture() {
        let test_context = TestContext::with_file(
            "<test>/test.py",
            r"
                def test_all_scopes(
                    missing_fixture: int,
                ) -> None:
                    assert missing_fixture == 1
                ",
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_failed();

        assert_eq!(*result.stats(), expected_stats);

        assert!(result.diagnostics().len() == 1);
    }

    #[rstest]
    fn test_parametrize_multiple_args_single_string(#[values("pytest", "karva")] framework: &str) {
        let test_context = TestContext::with_file(
            "<test>/test.py",
            &format!(
                r#"
                import {}

                @{}("input,expected", [
                    (2, 4),
                    (3, 9),
                ])
                def test_square(input, expected):
                    assert input ** 2 == expected
                "#,
                framework,
                get_parametrize_function(framework)
            ),
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats);

        assert!(result.diagnostics().is_empty());
    }

    #[rstest]
    fn test_temp_directory_fixture(
        #[values("tmp_path", "temp_path", "temp_dir")] fixture_name: &str,
    ) {
        let test_context = TestContext::with_file(
            "<test>/test.py",
            &format!(
                r"
                import pathlib

                def test_temp_directory_fixture({fixture_name}):
                    assert {fixture_name}.exists()
                    assert {fixture_name}.is_dir()
                    assert {fixture_name}.is_absolute()
                    assert isinstance({fixture_name}, pathlib.Path)
                "
            ),
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats);

        assert!(result.diagnostics().is_empty());
    }

    #[test]
    fn test_fixture_request() {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            r"
                import karva

                @karva.fixture
                def my_fixture(request):
                    # request should be a FixtureRequest instance with a param property
                    assert hasattr(request, 'param')
                    # For non-parametrized fixtures, param should be None
                    assert request.param is None
                    return 'fixture_value'

                def test_with_request_fixture(my_fixture):
                    assert my_fixture == 'fixture_value'
",
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        expected_stats.add_passed();

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrized_fixture() {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            r"
                import karva

                @karva.fixture(params=['a', 'b', 'c'])
                def my_fixture(request):
                    assert hasattr(request, 'param')
                    assert request.param in ['a', 'b', 'c']
                    return request.param

                def test_with_parametrized_fixture(my_fixture):
                    assert my_fixture in ['a', 'b', 'c']
",
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..3 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrized_fixture_in_conftest() {
        let test_context = TestContext::with_files([
            (
                "<test>/conftest.py",
                r"
                    import karva

                    @karva.fixture(params=[1, 2, 3])
                    def number_fixture(request):
                        return request.param * 10
                ",
            ),
            (
                "<test>/test_file.py",
                r"
                    def test_with_number(number_fixture):
                        assert number_fixture in [10, 20, 30]
                ",
            ),
        ]);

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..3 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrized_fixture_module_scope() {
        let test_context = TestContext::with_files([
            (
                "<test>/conftest.py",
                r"
                    import karva

                    @karva.fixture(scope='module', params=['x', 'y'])
                    def module_fixture(request):
                        return request.param.upper()
                ",
            ),
            (
                "<test>/test_file.py",
                r"
                    def test_first(module_fixture):
                        assert module_fixture in ['X', 'Y']

                    def test_second(module_fixture):
                        assert module_fixture in ['X', 'Y']
                ",
            ),
        ]);

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        // 2 tests * 2 params = 4 test runs
        for _ in 0..4 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrized_fixture_with_generator() {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            r"
                import karva

                results = []

                @karva.fixture(params=['setup_a', 'setup_b'])
                def setup_fixture(request):
                    value = request.param
                    results.append(f'{value}_start')
                    yield value
                    results.append(f'{value}_end')

                def test_with_setup(setup_fixture):
                    assert setup_fixture in ['setup_a', 'setup_b']
                    assert len(results) >= 1

                def test_verify_finalizers_ran():
                    # This test runs after the parametrized tests
                    # Both finalizers should have run
                    assert 'setup_a_start' in results
                    assert 'setup_a_end' in results
                    assert 'setup_b_start' in results
                    assert 'setup_b_end' in results
",
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..3 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrized_fixture_session_scope() {
        let test_context = TestContext::with_files([
            (
                "<test>/conftest.py",
                r"
                    import karva

                    call_count = []

                    @karva.fixture(scope='session', params=['session_1', 'session_2'])
                    def session_fixture(request):
                        call_count.append(request.param)
                        return request.param
                ",
            ),
            (
                "<test>/test_a.py",
                r"
                    def test_a1(session_fixture):
                        assert session_fixture in ['session_1', 'session_2']

                    def test_a2(session_fixture):
                        assert session_fixture in ['session_1', 'session_2']
                ",
            ),
            (
                "<test>/test_b.py",
                r"
                    def test_b1(session_fixture):
                        assert session_fixture in ['session_1', 'session_2']
                ",
            ),
        ]);

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..6 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrized_fixture_with_multiple_params() {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            r"
                import karva

                @karva.fixture(params=[10, 20])
                def number(request):
                    return request.param

                @karva.fixture(params=['a', 'b'])
                def letter(request):
                    return request.param

                def test_combination(number, letter):
                    assert number in [10, 20]
                    assert letter in ['a', 'b']
",
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..4 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrized_fixture_with_regular_parametrize() {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            r"
                import karva

                @karva.fixture(params=[1, 2])
                def fixture_param(request):
                    return request.param

                @karva.tags.parametrize('test_param', [10, 20])
                def test_both(fixture_param, test_param):
                    assert fixture_param in [1, 2]
                    assert test_param in [10, 20]
",
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..4 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrized_generator_fixture_finalizer_order() {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            r"
                import karva

                execution_log = []

                @karva.fixture(params=['first', 'second'])
                def ordered_fixture(request):
                    execution_log.append(f'{request.param}_setup')
                    yield request.param
                    execution_log.append(f'{request.param}_teardown')

                def test_one(ordered_fixture):
                    execution_log.append(f'test_one_{ordered_fixture}')
                    assert ordered_fixture in ['first', 'second']

                def test_check_order():
                    assert 'first_setup' in execution_log
                    assert 'test_one_first' in execution_log
                    assert 'first_teardown' in execution_log
                    assert 'second_setup' in execution_log
                    assert 'test_one_second' in execution_log
                    assert 'second_teardown' in execution_log

                    first_test_idx = execution_log.index('test_one_first')
                    first_teardown_idx = execution_log.index('first_teardown')
                    assert first_teardown_idx > first_test_idx
",
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..3 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrized_fixture_package_scope() {
        let test_context = TestContext::with_files([
            (
                "<test>/package/conftest.py",
                r"
                    import karva

                    @karva.fixture(scope='package', params=['pkg_a', 'pkg_b'])
                    def package_fixture(request):
                        return request.param
                ",
            ),
            (
                "<test>/package/test_one.py",
                r"
                    def test_in_one(package_fixture):
                        assert package_fixture in ['pkg_a', 'pkg_b']
                ",
            ),
            (
                "<test>/package/test_two.py",
                r"
                    def test_in_two(package_fixture):
                        assert package_fixture in ['pkg_a', 'pkg_b']
                ",
            ),
        ]);

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..4 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrized_fixture_with_dependency() {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            r"
                import karva

                @karva.fixture(params=[1, 2])
                def base_fixture(request):
                    return request.param

                @karva.fixture
                def dependent_fixture(base_fixture):
                    return base_fixture * 100

                def test_dependent(dependent_fixture):
                    assert dependent_fixture in [100, 200]
",
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..2 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }

    #[test]
    fn test_parametrized_fixture_finalizer_with_state() {
        let test_context = TestContext::with_file(
            "<test>/test_file.py",
            r"
                import karva

                arr = []

                @karva.fixture(params=['resource_1', 'resource_2', 'resource_3'])
                def resource(request):
                    resource_name = request.param
                    yield resource_name
                    arr.append(resource_name)

                def test_uses_resource(resource):
                    assert resource in ['resource_1', 'resource_2', 'resource_3']

                def test_all_cleaned_up():
                    assert arr == ['resource_1', 'resource_2', 'resource_3']
",
        );

        let result = test_context.test();

        let mut expected_stats = TestResultStats::default();

        for _ in 0..4 {
            expected_stats.add_passed();
        }

        assert_eq!(*result.stats(), expected_stats, "{result:?}");
    }
}
