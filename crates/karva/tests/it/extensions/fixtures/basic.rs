use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;
use crate::extensions::{get_auto_use_kw, get_parametrize_function};

#[rstest]
fn test_shared_dependency_is_initialized_once(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
import {framework}

calls = []

@{framework}.fixture
def shared():
    calls.append("setup")
    return []

@{framework}.fixture
def left(shared):
    return shared

@{framework}.fixture
def right(shared):
    return shared

def test_diamond(left, right, shared):
    assert left is right is shared
    assert calls == ["setup"]
"#
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_diamond(left=[], right=[], shared=[])
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn test_module_fixture_from_root_conftest() {
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

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_1(x=1)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_fixture_with_name_parameter(fixture_name=1)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_parent_package_fixture_lives_through_descendant_packages() {
    let context = TestContext::with_files([
        (
            "parent/conftest.py",
            r#"
from pathlib import Path

import karva

LOG = Path("lifecycle.log")
LOG.write_text("")

@karva.fixture(scope="package")
def parent_fixture():
    with LOG.open("a") as stream:
        stream.write("setup\n")
    yield "parent"
    with LOG.open("a") as stream:
        stream.write("teardown\n")
"#,
        ),
        (
            "parent/first/test_first.py",
            r#"
from pathlib import Path

def test_first(parent_fixture):
    assert parent_fixture == "parent"
    assert Path("lifecycle.log").read_text() == "setup\n"
"#,
        ),
        (
            "parent/second/test_second.py",
            r#"
from pathlib import Path

def test_second(parent_fixture):
    assert parent_fixture == "parent"
    assert Path("lifecycle.log").read_text() == "setup\n"
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] parent.first.test_first::test_first(parent_fixture='parent')
            PASS [TIME] parent.second.test_second::test_second(parent_fixture='parent')
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
    let lifecycle = std::fs::read_to_string(context.root().join("lifecycle.log"))
        .expect("read fixture lifecycle log");
    assert_eq!(lifecycle.lines().collect::<Vec<_>>(), ["setup", "teardown"]);
}

#[test]
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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_fixture(fixture=<test.Testcontext object at...)
            PASS [TIME] test::test_fixture_2(fixture=<test.Testcontext object at...)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] tests.test::test_1(x=1)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] tests.test::test_1(x=1)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] tests.test::test_1(x=1)
            PASS [TIME] tests.test::test_2(x=1)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] tests.test::test_1(x=1)
            PASS [TIME] tests.test::test_2(x=1)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
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

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] tests.test::test_1(x=1)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn test_dynamic_fixture_scope_session_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
from {framework} import fixture

def dynamic_scope(fixture_name, config):
    assert fixture_name == "x_session"
    assert config is None
    return "session"

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
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] test::test_1(x_session=[])
                PASS [TIME] test::test_2(x_session=[1])
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ")
    };
}

#[rstest]
fn test_dynamic_fixture_scope_function_scope(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
from {framework} import fixture

def dynamic_scope(fixture_name, config):
    assert fixture_name == "x_function"
    assert config is None
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
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] test::test_1(x_function=[])
                PASS [TIME] test::test_2(x_function=[])
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
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

    assert_cmd_snapshot!(context.command().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_nearest_conftest_fixture_shadows_outer_fixture() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import karva

@karva.fixture
def database():
    return "outer"
"#,
        ),
        (
            "nested/conftest.py",
            r#"
import karva

@karva.fixture
def database(database):
    return "inner-" + database
"#,
        ),
        (
            "nested/test_example.py",
            r#"
def test_database(database):
    assert database == "inner-outer"
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] nested.test_example::test_database(database='inner-outer')
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_nested_conftest_fixture_overrides_chain_to_each_outer_fixture() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import karva

@karva.fixture
def database():
    return "root"
"#,
        ),
        (
            "nested/conftest.py",
            r#"
import karva

@karva.fixture
def database(database):
    return "nested-" + database
"#,
        ),
        (
            "nested/inner/conftest.py",
            r#"
import karva

@karva.fixture
def database(database):
    return "inner-" + database
"#,
        ),
        (
            "nested/inner/test_example.py",
            r#"
def test_database(database):
    assert database == "inner-nested-root"
"#,
        ),
        (
            "nested/inner/test_local.py",
            r#"
import karva

@karva.fixture
def database(database):
    return "local-" + database

def test_database(database):
    assert database == "local-inner-nested-root"
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] nested.inner.test_example::test_database(database='inner-nested-root')
            PASS [TIME] nested.inner.test_local::test_database(database='local-inner-nested-root')
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_same_named_session_fixtures_do_not_share_values() {
    let context = TestContext::with_files([
        (
            "test_alpha.py",
            r#"
import karva

@karva.fixture(scope="session")
def value():
    return "alpha"

def test_value(value):
    assert value == "alpha"
"#,
        ),
        (
            "test_beta.py",
            r#"
import karva

@karva.fixture(scope="session")
def value():
    return "beta"

def test_value(value):
    assert value == "beta"
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_alpha::test_value(value='alpha')
            PASS [TIME] test_beta::test_value(value='beta')
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_test_keyword_default_is_not_a_fixture_request() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.fixture
def value():
    raise AssertionError("defaulted parameter was resolved as a fixture")

def test_default(value="default"):
    assert value == "default"
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_default
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_test_positional_only_default_is_not_a_fixture_request() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.fixture
def value():
    raise AssertionError("defaulted parameter was resolved as a fixture")

def test_default(value="default", /):
    assert value == "default"
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_default
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixture_keyword_default_is_not_a_fixture_request() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.fixture
def value():
    raise AssertionError("defaulted parameter was resolved as a fixture")

@karva.fixture
def configured(value="default"):
    return value

def test_default(configured):
    assert configured == "default"
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_default(configured='default')
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixture_positional_only_default_is_not_a_fixture_request() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

@karva.fixture
def value():
    raise AssertionError("defaulted parameter was resolved as a fixture")

@karva.fixture
def configured(value="default", /):
    return value

def test_default(configured):
    assert configured == "default"
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_default(configured='default')
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
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
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_all_scopes(session_fixture=1, module_fixture=2, package_fixture=3, function_fixture=4)
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
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
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_calculator(calculator=<test.Calculator object at ...)
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_order_respects_scope(#[values("pytest", "karva")] framework: &str) {
    let auto_use_kw = get_auto_use_kw(framework);
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
                "
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_value(clean_data=None)
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
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

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_file::test_z(y=0)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_pytest_named_fixture() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.fixture(name="custom_name")
def original_name():
    return 42

def test_named_fixture(custom_name):
    assert custom_name == 42
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_named_fixture(custom_name=42)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_function_scoped_fixture_isolated_per_test() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.fixture
def make_subdir(tmp_path):
    def _make(name):
        d = tmp_path / name
        d.mkdir()
        return d
    return _make

def test_first(make_subdir):
    d = make_subdir('subdir')
    assert d.exists()

def test_second(make_subdir):
    # Fresh tmp_path per test — mkdir on same name must not raise FileExistsError
    d = make_subdir('subdir')
    assert d.exists()

def test_third(make_subdir):
    d = make_subdir('subdir')
    assert d.exists()
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_pytest_fixture_imported_into_conftest() {
    let context = TestContext::with_files([
        ("mypackage/__init__.py", ""),
        (
            "mypackage/fixtures.py",
            r"
import pytest

@pytest.fixture
def invoke():
    return 'invoked'
",
        ),
        ("conftest.py", "from mypackage.fixtures import invoke"),
        (
            "test_invoke.py",
            "def test_invoke(invoke): assert invoke == 'invoked'",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_invoke::test_invoke(invoke='invoked')
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_config_try_import_fixtures() {
    let context = TestContext::with_files([
        (
            "pyproject.toml",
            r"
[tool.karva.profile.default.test]
try-import-fixtures = true
",
        ),
        (
            "fixtures.py",
            r#"
import karva

@karva.fixture
def imported_fixture():
    return "imported"
"#,
        ),
        (
            "test_imported_fixture.py",
            r#"
from fixtures import imported_fixture

def test_uses_imported_fixture(imported_fixture):
    assert imported_fixture == "imported"
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_imported_fixture::test_uses_imported_fixture(imported_fixture='imported')
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
#[cfg(unix)]
fn test_imported_fixture_missing_source_is_reported() {
    let context = TestContext::with_files([
        ("mypackage/__init__.py", ""),
        (
            "mypackage/fixtures.py",
            r"
import pytest

@pytest.fixture
def invoke():
    return 'invoked'
",
        ),
        (
            "conftest.py",
            r"from pathlib import Path

from mypackage.fixtures import invoke

Path(__file__).with_name('mypackage').joinpath('fixtures.py').unlink()
",
        ),
        (
            "test_invoke.py",
            "def test_invoke(invoke): assert invoke == 'invoked'",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test_invoke::test_invoke

    failures:

    test_invoke::test_invoke:

    error[missing-fixtures]: Test `test_invoke` has missing fixtures
     --> test_invoke.py:1:5
      |
    1 | def test_invoke(invoke): assert invoke == 'invoked'
      |     ^^^^^^^^^^^
    info: Missing fixtures: `invoke`

    diagnostics:

    error[failed-to-discover-imported-fixture]: Failed to discover imported fixture `invoke` from `<temp_dir>/mypackage/fixtures.py`: failed to open file `<temp_dir>/mypackage/fixtures.py`: No such file or directory (os error 2)

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn test_fixture_basic(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def my_fixture():
                    return 'value'

                def test_with_fixture(my_fixture):
                    assert my_fixture == 'value'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_with_fixture(my_fixture='value')
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_in_conftest(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_files([
        (
            "conftest.py",
            format!(
                r"
                    import {framework}

                    @{framework}.fixture
                    def number_fixture():
                        return 42
                "
            )
            .as_str(),
        ),
        (
            "test.py",
            r"
                    def test_with_number(number_fixture):
                        assert number_fixture == 42
                ",
        ),
    ]);

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_with_number(number_fixture=42)
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_with_multiple_fixtures(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def number():
                    return 100

                @{framework}.fixture
                def letter():
                    return 'X'

                def test_combination(number, letter):
                    assert number == 100
                    assert letter == 'X'
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_combination(number=100, letter='X')
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_with_test_parametrize(#[values("pytest", "karva")] framework: &str) {
    let parametrize = get_parametrize_function(framework);
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def fixture_value():
                    return 'fixture_value'

                @{parametrize}('test_param', [10, 20])
                def test_both(fixture_value, test_param):
                    assert fixture_value == 'fixture_value'
                    assert test_param in [10, 20]
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_both(fixture_value='fixture_value', test_param=10)
                PASS [TIME] test::test_both(fixture_value='fixture_value', test_param=20)
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_with_dependency(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                @{framework}.fixture
                def base_fixture():
                    return 10

                @{framework}.fixture
                def dependent_fixture(base_fixture):
                    return base_fixture * 100

                def test_dependent(dependent_fixture):
                    assert dependent_fixture == 1000
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_dependent(dependent_fixture=1000)
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}
