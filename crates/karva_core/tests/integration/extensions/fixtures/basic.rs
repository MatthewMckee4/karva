use insta::{allow_duplicates, assert_snapshot};
use karva_test::TestContext;
use rstest::rstest;

use crate::common::TestRunnerExt;

#[test]
fn test_fixture_manager_add_fixtures_impl_three_dependencies_different_scopes_with_fixture_in_function()
 {
    let context = TestContext::with_files([
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

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_runner_given_nested_path() {
    let context = TestContext::with_files([
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

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_fixture_with_name_parameter() {
    let context = TestContext::with_file(
        "<test>/test_file.py",
        r#"import karva

@karva.fixture(name="fixture_name")
def fixture_1():
    return 1

def test_fixture_with_name_parameter(fixture_name):
    assert fixture_name == 1
"#,
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_fixture_is_different_in_different_functions() {
    let context = TestContext::with_file(
        "<test>/test_file.py",
        r"import karva

class Testcontext:
    def __init__(self):
        self.x = 1

@karva.fixture
def fixture():
    return Testcontext()

def test_fixture(fixture):
    assert fixture.x == 1
    fixture.x = 2

def test_fixture_2(fixture):
    assert fixture.x == 1
    fixture.x = 2
",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_fixture_from_current_package_session_scope() {
    let context = TestContext::with_files([
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

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_fixture_from_current_package_function_scope() {
    let context = TestContext::with_files([
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

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_finalizer_from_current_package_session_scope() {
    let context = TestContext::with_files([
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

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_finalizer_from_current_package_function_scope() {
    let context = TestContext::with_files([
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

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_discover_pytest_fixture() {
    let context = TestContext::with_files([
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

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[rstest]
fn test_dynamic_fixture_scope_session_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
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

    let result = context.test();

    allow_duplicates!(
        assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]")
    );
}

#[rstest]
fn test_dynamic_fixture_scope_function_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[test]
fn test_fixture_override_in_test_modules() {
    let context = TestContext::with_files([
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

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[rstest]
fn test_fixture_initialization_order(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
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
    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[test]
fn test_invalid_pytest_fixture_scope() {
    let context = TestContext::with_file(
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

    let result = context.test();

    assert_snapshot!(result.display(), @r#"
    discovery failures:

    invalid fixture `some_fixture`: Invalid fixture scope: sessionss at <temp_dir>/<test>/test.py:4

    test failures:

    test `<test>.test::test_all_scopes` has missing fixtures: ["some_fixture"] at <temp_dir>/<test>/test.py:8

    test failures:
        <test>.test::test_all_scopes at <temp_dir>/<test>/test.py:8

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    "#);

    assert!(result.total_diagnostics() == 2);
}

#[test]
fn test_missing_fixture() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
                def test_all_scopes(
                    missing_fixture: int,
                ) -> None:
                    assert missing_fixture == 1
                ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r#"
    test failures:

    test `<test>.test::test_all_scopes` has missing fixtures: ["missing_fixture"] at <temp_dir>/<test>/test.py:2

    test failures:
        <test>.test::test_all_scopes at <temp_dir>/<test>/test.py:2

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    "#);

    assert!(result.diagnostics().len() == 1);
}

#[rstest]
fn test_nested_generator_fixture(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[test]
fn test_fixture_fails_to_run() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
                from karva import fixture

                @fixture
                def failing_fixture():
                    raise Exception('Fixture failed')

                def test_failing_fixture(failing_fixture):
                    pass
                ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r#"
    fixture failures:

    fixture function `<test>.test::failing_fixture` at <temp_dir>/<test>/test.py:4 failed at <temp_dir>/<test>/test.py:6
    Fixture failed

    test failures:

    test `<test>.test::test_failing_fixture` has missing fixtures: ["failing_fixture"] at <temp_dir>/<test>/test.py:8

    test failures:
        <test>.test::test_failing_fixture at <temp_dir>/<test>/test.py:8

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    "#);
}

#[test]
fn test_fixture_missing_fixtures() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
                from karva import fixture

                @fixture
                def failing_fixture(missing_fixture):
                    return 1

                def test_failing_fixture(failing_fixture):
                    pass
                ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r#"
    fixture failures:

    fixture `<test>.test::failing_fixture` has missing fixtures: ["missing_fixture"] at <temp_dir>/<test>/test.py:4

    test failures:

    test `<test>.test::test_failing_fixture` has missing fixtures: ["failing_fixture"] at <temp_dir>/<test>/test.py:8

    test failures:
        <test>.test::test_failing_fixture at <temp_dir>/<test>/test.py:8

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    "#);
}
