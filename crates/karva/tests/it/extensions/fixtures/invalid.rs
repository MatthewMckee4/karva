use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[test]
fn test_invalid_pytest_fixture_scope() {
    let context = TestContext::with_file(
        "test.py",
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

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_all_scopes

    failures:

    test::test_all_scopes:

    error[missing-fixtures]: Test `test_all_scopes` has missing fixtures
     --> test.py:8:5
      |
    8 | def test_all_scopes(
      |     ^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `some_fixture`

    diagnostics:

    error[invalid-fixture]: Discovered an invalid fixture `some_fixture`
     --> test.py:5:5
      |
    5 | def some_fixture() -> int:
      |     ^^^^^^^^^^^^
      |
    info: Invalid fixture scope `sessionss`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_invalid_karva_fixture_scope() {
    let context = TestContext::with_file(
        "test.py",
        r#"import karva

@karva.fixture(scope="sessionss")
def some_fixture() -> int:
    return 1

def test_all_scopes(some_fixture: int) -> None:
    assert some_fixture == 1
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_all_scopes

    failures:

    test::test_all_scopes:

    error[missing-fixtures]: Test `test_all_scopes` has missing fixtures
     --> test.py:7:5
      |
    7 | def test_all_scopes(some_fixture: int) -> None:
      |     ^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `some_fixture`

    diagnostics:

    error[invalid-fixture]: Discovered an invalid fixture `some_fixture`
     --> test.py:4:5
      |
    4 | def some_fixture() -> int:
      |     ^^^^^^^^^^^^
      |
    info: Invalid fixture scope `sessionss`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_duplicate_fixture_names_are_rejected_without_running_body() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
import pytest
from pathlib import Path

def mark(name):
    Path(name).write_text("ran")

fixture_name = "shared"

@karva.fixture(name=fixture_name, auto_use=True)
def first_fixture():
    mark("first.txt")

@pytest.fixture(name=fixture_name, autouse=True)
def second_fixture():
    mark("second.txt")

def test_ok():
    assert True
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_ok

    diagnostics:

    error[duplicate-fixture]: Fixture `shared` is defined more than once
      --> test.py:16:5
       |
    16 | def second_fixture():
       |     ^^^^^^^^^^^^^^
       |
    info: First definition of `shared` is here
      --> test.py:12:5
       |
    12 | def first_fixture():
       |     ^^^^^^^^^^^^^
       |

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    for marker in ["first.txt", "second.txt"] {
        assert!(!context.root().join(marker).exists());
    }
}

#[test]
fn test_missing_fixture() {
    let context = TestContext::with_file(
        "test.py",
        r"
                def test_all_scopes(
                    missing_fixture: int,
                ) -> None:
                    assert missing_fixture == 1
                ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_all_scopes

    failures:

    test::test_all_scopes:

    error[missing-fixtures]: Test `test_all_scopes` has missing fixtures
     --> test.py:2:5
      |
    2 | def test_all_scopes(
      |     ^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `missing_fixture`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixture_fails_to_run() {
    let context = TestContext::with_file(
        "test.py",
        r"
                from karva import fixture

                @fixture
                def failing_fixture():
                    raise Exception('Fixture failed')

                def test_failing_fixture(failing_fixture):
                    pass
                ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_failing_fixture

    failures:

    test::test_failing_fixture (requires fixture `failing_fixture`):

    error[fixture-failure]: Fixture `failing_fixture` failed
     --> test.py:5:5
      |
    5 | def failing_fixture():
      |     ^^^^^^^^^^^^^^^
      |
    info: Fixture failed here
     --> test.py:6:5
      |
    6 |     raise Exception('Fixture failed')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Fixture failed

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixture_missing_fixtures() {
    let context = TestContext::with_file(
        "test.py",
        r"
                from karva import fixture
                from pathlib import Path

                @fixture
                def failing_fixture(missing_fixture):
                    Path('fixture-ran').touch()
                    return 1

                def test_failing_fixture(failing_fixture):
                    pass
                ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_failing_fixture

    failures:

    test::test_failing_fixture:

    error[missing-fixtures]: Fixture `failing_fixture` has missing fixtures
     --> test.py:6:5
      |
    6 | def failing_fixture(missing_fixture):
      |     ^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `missing_fixture`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("fixture-ran").exists());
}

#[test]
fn test_aliased_fixture_missing_fixtures_uses_source_name() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.fixture(name="aliased_fixture")
def source_fixture(missing_fixture):
    return 1

def test_fixture(aliased_fixture):
    pass
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_fixture

    failures:

    test::test_fixture:

    error[missing-fixtures]: Fixture `source_fixture` has missing fixtures
     --> test.py:5:5
      |
    5 | def source_fixture(missing_fixture):
      |     ^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `missing_fixture`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn missing_arguments_in_nested_function() {
    let context = TestContext::with_file(
        "test.py",
        r"
                def test_failing_fixture():

                    def inner(missing_fixture): ...

                    inner()
                   ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_failing_fixture

    failures:

    test::test_failing_fixture:

    error[test-failure]: Test `test_failing_fixture` failed
     --> test.py:2:5
      |
    2 | def test_failing_fixture():
      |     ^^^^^^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:6:5
      |
    6 |     inner()
      |     ^^^^^^^
      |
    info: test_failing_fixture.<locals>.inner() missing 1 required positional argument: 'missing_fixture'

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_failing_yield_fixture() {
    let context = TestContext::with_file(
        "test.py",
        r"
            import karva

            @karva.fixture
            def fixture():
                def foo():
                    raise ValueError('foo')
                yield foo()

            def test_failing_fixture(fixture):
                assert True
                   ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_failing_fixture

    failures:

    test::test_failing_fixture (requires fixture `fixture`):

    error[fixture-failure]: Fixture `fixture` failed
     --> test.py:5:5
      |
    5 | def fixture():
      |     ^^^^^^^
      |
    info: Fixture failed here
     --> test.py:7:9
      |
    7 |         raise ValueError('foo')
      |         ^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: foo

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixture_generator_two_yields() {
    let context = TestContext::with_file(
        "test.py",
        r"
                import karva

                @karva.fixture
                def fixture_generator():
                    yield 1
                    yield 2

                def test_fixture_generator(fixture_generator):
                    assert fixture_generator == 1
                ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_fixture_generator(fixture_generator=1)

    failures:

    test::test_fixture_generator(fixture_generator=1):

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `fixture_generator`
     --> test.py:5:5
      |
    5 | def fixture_generator():
      |     ^^^^^^^^^^^^^^^^^
      |
    info: Fixture had more than one yield statement

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixture_generator_fail_in_teardown() {
    let context = TestContext::with_file(
        "test.py",
        r#"
                import karva

                @karva.fixture
                def fixture_generator():
                    yield 1
                    raise ValueError("fixture-error")

                def test_fixture_generator(fixture_generator):
                    assert fixture_generator == 1
                "#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_fixture_generator(fixture_generator=1)

    failures:

    test::test_fixture_generator(fixture_generator=1):

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `fixture_generator`
     --> test.py:5:5
      |
    5 | def fixture_generator():
      |     ^^^^^^^^^^^^^^^^^
      |
    info: Failed to reset fixture: fixture-error

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixture_dependency_chain_failure() {
    let context = TestContext::with_file(
        "test.py",
        r"
                from karva import fixture

                @fixture
                def config():
                    raise Exception('config failed')

                @fixture
                def connection(config):
                    return config

                @fixture
                def db(connection):
                    return connection

                def test_with_db(db):
                    pass
                ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_with_db

    failures:

    test::test_with_db (requires fixture `db`):

    error[fixture-failure]: Fixture `config` failed
     --> test.py:5:5
      |
    5 | def config():
      |     ^^^^^^
      |
    info: Fixture `db` requires `connection`
      --> test.py:13:5
       |
    13 | def db(connection):
       |     ^^
       |
    info: Fixture `connection` requires `config`
     --> test.py:9:5
      |
    9 | def connection(config):
      |     ^^^^^^^^^^
      |
    info: Fixture failed here
     --> test.py:6:5
      |
    6 |     raise Exception('config failed')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: config failed

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixture_scope_non_string_non_callable() {
    let context = TestContext::with_file(
        "test.py",
        r"import karva

@karva.fixture(scope=123)
def my_fixture():
    return 42

def test_with_fixture(my_fixture):
    assert my_fixture == 42
",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_with_fixture

    failures:

    test::test_with_fixture:

    error[missing-fixtures]: Test `test_with_fixture` has missing fixtures
     --> test.py:7:5
      |
    7 | def test_with_fixture(my_fixture):
      |     ^^^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `my_fixture`

    diagnostics:

    error[invalid-fixture]: Discovered an invalid fixture `my_fixture`
     --> test.py:4:5
      |
    4 | def my_fixture():
      |     ^^^^^^^^^^
      |
    info: Scope must be either a string or a callable

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixture_reports_rejected_dependency() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
from pathlib import Path

@karva.fixture(scope="invalid")
def config():
    return {}

@karva.fixture
def service(config):
    Path("fixture-ran").touch()
    return config

def test_service(service):
    Path("test-ran").touch()
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_service

    failures:

    test::test_service:

    error[missing-fixtures]: Fixture `service` has missing fixtures
      --> test.py:10:5
       |
    10 | def service(config):
       |     ^^^^^^^
       |
    info: Missing fixtures: `config`
    info: Fixture `config` was rejected during discovery: Invalid fixture scope `invalid`
     --> test.py:6:5
      |
    6 | def config():
      |     ^^^^^^
      |

    diagnostics:

    error[invalid-fixture]: Discovered an invalid fixture `config`
     --> test.py:6:5
      |
    6 | def config():
      |     ^^^^^^
      |
    info: Invalid fixture scope `invalid`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("fixture-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[test]
fn test_fixture_reports_rejected_dependency_from_conftest() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import karva

@karva.fixture(scope="invalid")
def config():
    return {}
"#,
        ),
        (
            "test.py",
            r#"
import karva
from pathlib import Path

@karva.fixture
def service(config):
    Path("fixture-ran").touch()

def test_service(service):
    Path("test-ran").touch()
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_service

    failures:

    test::test_service:

    error[missing-fixtures]: Fixture `service` has missing fixtures
     --> test.py:6:5
      |
    6 | def service(config):
      |     ^^^^^^^
      |
    info: Missing fixtures: `config`
    info: Fixture `config` was rejected during discovery: Invalid fixture scope `invalid`
     --> conftest.py:5:5
      |
    5 | def config():
      |     ^^^^^^
      |

    diagnostics:

    error[invalid-fixture]: Discovered an invalid fixture `config`
     --> conftest.py:5:5
      |
    5 | def config():
      |     ^^^^^^
      |
    info: Invalid fixture scope `invalid`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("fixture-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[test]
fn test_rejected_module_fixture_shadows_valid_conftest_fixture() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import karva
from pathlib import Path

@karva.fixture
def config():
    Path("parent-ran").touch()
    return {}
"#,
        ),
        (
            "test.py",
            r#"
import karva
from pathlib import Path

@karva.fixture(scope="invalid")
def config():
    return {}

@karva.fixture
def service(config):
    Path("fixture-ran").touch()

def test_service(service):
    Path("test-ran").touch()
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_service

    failures:

    test::test_service:

    error[missing-fixtures]: Fixture `service` has missing fixtures
      --> test.py:10:5
       |
    10 | def service(config):
       |     ^^^^^^^
       |
    info: Missing fixtures: `config`
    info: Fixture `config` was rejected during discovery: Invalid fixture scope `invalid`
     --> test.py:6:5
      |
    6 | def config():
      |     ^^^^^^
      |

    diagnostics:

    error[invalid-fixture]: Discovered an invalid fixture `config`
     --> test.py:6:5
      |
    6 | def config():
      |     ^^^^^^
      |
    info: Invalid fixture scope `invalid`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("parent-ran").exists());
    assert!(!context.root().join("fixture-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[test]
fn test_valid_module_fixture_shadows_rejected_conftest_fixture() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import karva

@karva.fixture(scope="invalid")
def config():
    return {}
"#,
        ),
        (
            "test.py",
            r#"
import karva
from pathlib import Path

@karva.fixture
def config():
    Path("fixture-ran").touch()
    return {}

def test_config(config):
    Path("test-ran").touch()
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_config(config={})

    diagnostics:

    error[invalid-fixture]: Discovered an invalid fixture `config`
     --> conftest.py:5:5
      |
    5 | def config():
      |     ^^^^^^
      |
    info: Invalid fixture scope `invalid`

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
    assert!(context.root().join("fixture-ran").exists());
    assert!(context.root().join("test-ran").exists());
}

#[test]
fn test_fixture_reports_multiple_rejected_and_unknown_dependencies() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
from pathlib import Path

@karva.fixture(scope="invalid-config")
def config():
    return {}

@karva.fixture(scope="invalid-credentials")
def credentials():
    return {}

@karva.fixture
def service(config, unknown, credentials):
    Path("fixture-ran").touch()

def test_service(service):
    Path("test-ran").touch()
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_service

    failures:

    test::test_service:

    error[missing-fixtures]: Fixture `service` has missing fixtures
      --> test.py:14:5
       |
    14 | def service(config, unknown, credentials):
       |     ^^^^^^^
       |
    info: Missing fixtures: `config`, `unknown`, `credentials`
    info: Fixture `config` was rejected during discovery: Invalid fixture scope `invalid-config`
     --> test.py:6:5
      |
    6 | def config():
      |     ^^^^^^
      |
    info: Fixture `credentials` was rejected during discovery: Invalid fixture scope `invalid-credentials`
      --> test.py:10:5
       |
    10 | def credentials():
       |     ^^^^^^^^^^^
       |

    diagnostics:

    error[invalid-fixture]: Discovered an invalid fixture `config`
     --> test.py:6:5
      |
    6 | def config():
      |     ^^^^^^
      |
    info: Invalid fixture scope `invalid-config`

    error[invalid-fixture]: Discovered an invalid fixture `credentials`
      --> test.py:10:5
       |
    10 | def credentials():
       |     ^^^^^^^^^^^
       |
    info: Invalid fixture scope `invalid-credentials`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("fixture-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[test]
fn test_fixture_reports_rejected_imported_dependency_alias() {
    let context = TestContext::with_files([
        (
            "fixture_helpers.py",
            r#"
import pytest

@pytest.fixture(scope="invalid")
def original_config():
    return {}
"#,
        ),
        (
            "conftest.py",
            "from fixture_helpers import original_config as config\n",
        ),
        (
            "test.py",
            r#"
import karva
from pathlib import Path

@karva.fixture
def service(config):
    Path("fixture-ran").touch()

def test_service(service):
    Path("test-ran").touch()
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_service

    failures:

    test::test_service:

    error[missing-fixtures]: Fixture `service` has missing fixtures
     --> test.py:6:5
      |
    6 | def service(config):
      |     ^^^^^^^
      |
    info: Missing fixtures: `config`
    info: Fixture `config` was rejected during discovery: Invalid fixture scope `invalid`
     --> fixture_helpers.py:5:5
      |
    5 | def original_config():
      |     ^^^^^^^^^^^^^^^
      |

    diagnostics:

    error[invalid-fixture]: Discovered an invalid fixture `original_config`
     --> fixture_helpers.py:5:5
      |
    5 | def original_config():
      |     ^^^^^^^^^^^^^^^
      |
    info: Invalid fixture scope `invalid`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("fixture-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[rstest]
fn test_fixture_scope_mismatch(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
import {framework} as framework
from pathlib import Path

@framework.fixture
def connection():
    Path("connection-ran").touch()

@framework.fixture(scope="session")
def database(connection):
    Path("database-ran").touch()

def test_database(database):
    Path("test-ran").touch()
"#,
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test::test_database

        failures:

        test::test_database:

        error[fixture-scope-mismatch]: Fixture `database` with `session` scope cannot depend on fixture `connection` with `function` scope
          --> test.py:10:5
           |
        10 | def database(connection):
           |     ^^^^^^^^
           |
        info: Fixture `connection` has `function` scope
         --> test.py:6:5
          |
        6 | def connection():
          |     ^^^^^^^^^^
          |

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        ");
    }
    assert!(!context.root().join("connection-ran").exists());
    assert!(!context.root().join("database-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[test]
fn test_nested_async_generator_autouse_fixture_scope_mismatch() {
    let context = TestContext::with_files([
        (
            "nested/conftest.py",
            r#"
import karva
from pathlib import Path

@karva.fixture
def connection():
    Path("connection-ran").touch()

@karva.fixture(scope="package", auto_use=True)
async def database(connection):
    Path("database-ran").touch()
    yield
"#,
        ),
        (
            "nested/test.py",
            r#"
from pathlib import Path

def test_database():
    Path("test-ran").touch()
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] nested.test::test_database

    failures:

    nested.test::test_database:

    error[fixture-scope-mismatch]: Fixture `database` with `package` scope cannot depend on fixture `connection` with `function` scope
      --> nested/conftest.py:10:11
       |
    10 | async def database(connection):
       |           ^^^^^^^^
       |
    info: Fixture `connection` has `function` scope
     --> nested/conftest.py:6:5
      |
    6 | def connection():
      |     ^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("connection-ran").exists());
    assert!(!context.root().join("database-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[test]
fn test_fixture_scope_mismatch_reports_dependency_chain() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
from pathlib import Path

def dynamic_scope(*, fixture_name, config):
    return "function"

@karva.fixture(scope=dynamic_scope)
def connection():
    Path("fixture-ran").touch()

@karva.fixture(scope="session")
def repository(connection):
    Path("fixture-ran").touch()

@karva.fixture(scope="session")
def database(repository):
    Path("fixture-ran").touch()

def test_database(database):
    Path("test-ran").touch()
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_database

    failures:

    test::test_database:

    error[fixture-scope-mismatch]: Fixture `repository` with `session` scope cannot depend on fixture `connection` with `function` scope
      --> test.py:13:5
       |
    13 | def repository(connection):
       |     ^^^^^^^^^^
       |
    info: Fixture `database` depends on fixture `repository`
      --> test.py:17:5
       |
    17 | def database(repository):
       |     ^^^^^^^^
       |
    info: Fixture `connection` has `function` scope
     --> test.py:9:5
      |
    9 | def connection():
      |     ^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("fixture-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[test]
fn test_fixture_scope_mismatch_reports_first_invalid_edge() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
from pathlib import Path

@karva.fixture
def function_fixture():
    Path("fixture-ran").touch()

@karva.fixture(scope="module")
def module_fixture(function_fixture):
    Path("fixture-ran").touch()

@karva.fixture(scope="session")
def session_fixture(module_fixture):
    Path("fixture-ran").touch()

@karva.fixture(scope="package")
def package_fixture(session_fixture):
    Path("fixture-ran").touch()

def test_scopes(package_fixture):
    Path("test-ran").touch()
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_scopes

    failures:

    test::test_scopes:

    error[fixture-scope-mismatch]: Fixture `session_fixture` with `session` scope cannot depend on fixture `module_fixture` with `module` scope
      --> test.py:14:5
       |
    14 | def session_fixture(module_fixture):
       |     ^^^^^^^^^^^^^^^
       |
    info: Fixture `package_fixture` depends on fixture `session_fixture`
      --> test.py:18:5
       |
    18 | def package_fixture(session_fixture):
       |     ^^^^^^^^^^^^^^^
       |
    info: Fixture `module_fixture` has `module` scope
      --> test.py:10:5
       |
    10 | def module_fixture(function_fixture):
       |     ^^^^^^^^^^^^^^
       |

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
    assert!(!context.root().join("fixture-ran").exists());
    assert!(!context.root().join("test-ran").exists());
}
