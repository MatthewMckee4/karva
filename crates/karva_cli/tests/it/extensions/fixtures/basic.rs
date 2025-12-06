use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

fn get_auto_use_kw(framework: &str) -> &str {
    match framework {
        "pytest" => "autouse",
        "karva" => "auto_use",
        _ => panic!("Invalid framework"),
    }
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_manager_add_fixtures_impl_three_dependencies_different_scopes_with_fixture_in_function()
 {
    let context = TestContext::with_files([
        (
            "conftest.py",
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
        ("test.py", "def test_1(z): pass"),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_1(z=1) ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_runner_given_nested_path() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r"
import karva
@karva.fixture(scope='module')
def x():
    return 1
            ",
        ),
        ("test.py", "def test_1(x): pass"),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_1(x=1) ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_with_name_parameter() {
    let context = TestContext::with_file(
        "test.py",
        r#"import karva

@karva.fixture(name="fixture_name")
def fixture_1():
    return 1

def test_fixture_with_name_parameter(fixture_name):
    assert fixture_name == 1
"#,
    );

    assert_cmd_snapshot!(context.command(), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_fixture_with_name_parameter(fixture_name=1) ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_is_different_in_different_functions() {
    let context = TestContext::with_file(
        "test.py",
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

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_fixture(fixture=<test.Testcontext object at...) ... ok
    test test::test_fixture_2(fixture=<test.Testcontext object at...) ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_from_current_package_session_scope() {
    let context = TestContext::with_files([
        (
            "tests/conftest.py",
            r"
import karva

@karva.fixture(scope='session')
def x():
    return 1
            ",
        ),
        ("tests/test.py", "def test_1(x): pass"),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test tests.test::test_1(x=1) ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_from_current_package_function_scope() {
    let context = TestContext::with_files([
        (
            "tests/conftest.py",
            r"
import karva
@karva.fixture
def x():
    return 1
            ",
        ),
        ("tests/test.py", "def test_1(x): pass"),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test tests.test::test_1(x=1) ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_finalizer_from_current_package_session_scope() {
    let context = TestContext::with_files([
        (
            "tests/conftest.py",
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
            "tests/test.py",
            r"
from .conftest import arr

def test_1(x):
    assert len(arr) == 0

def test_2(x):
    assert len(arr) == 0
",
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test tests.test::test_1(x=1) ... ok
    test tests.test::test_2(x=1) ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_finalizer_from_current_package_function_scope() {
    let context = TestContext::with_files([
        (
            "tests/conftest.py",
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
            "tests/test.py",
            r"
from .conftest import arr

def test_1(x):
    assert len(arr) == 0

def test_2(x):
    assert len(arr) == 1
",
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test tests.test::test_1(x=1) ... ok
    test tests.test::test_2(x=1) ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_discover_pytest_fixture() {
    let context = TestContext::with_files([
        (
            "tests/conftest.py",
            r"
import pytest

@pytest.fixture
def x():
    return 1
",
        ),
        ("tests/test.py", "def test_1(x): pass"),
    ]);

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test tests.test::test_1(x=1) ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_dynamic_fixture_scope_session_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_1(x_session=[]) ... ok
        test test::test_2(x_session=[1]) ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ")
    };
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_dynamic_fixture_scope_function_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_1(x_function=[]) ... ok
        test test::test_2(x_function=[]) ... ok

        test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_override_in_test_modules() {
    let context = TestContext::with_files([
        (
            "tests/conftest.py",
            r"
import karva

@karva.fixture
def username():
    return 'username'
",
        ),
        (
            "tests/test_1.py",
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
            "tests/test_2.py",
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

    assert_cmd_snapshot!(context.command().arg("-q"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_initialization_order(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_all_scopes(function_fixture=4, module_fixture=2, package_fixture=3, session_fixture=1) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_nested_generator_fixture(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_calculator(calculator=<test.Calculator object at ...) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_order_respects_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_value(clean_data=None) ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_fixture_depends_on_fixture_with_finalizer() {
    let context = TestContext::with_file(
        "test_file.py",
        r"
import karva

arr = []

@karva.fixture
def x():
    yield len(arr)
    arr.append(1)

@karva.fixture
def y(x):
    yield x

def test_z(y):
    assert y == len(arr)
            ",
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_file::test_z(y=0) ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
