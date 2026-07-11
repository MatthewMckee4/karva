use insta_cmd::assert_cmd_snapshot;

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
            FAIL [TIME] test::test_all_scopes

    diagnostics:

    error[invalid-fixture]: Discovered an invalid fixture `some_fixture`
     --> test.py:5:5
      |
    5 | def some_fixture() -> int:
      |     ^^^^^^^^^^^^
      |
    info: Invalid fixture scope: sessionss

    error[missing-fixtures]: Test `test_all_scopes` has missing fixtures
     --> test.py:8:5
      |
    8 | def test_all_scopes(
      |     ^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `some_fixture`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

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
            FAIL [TIME] test::test_all_scopes

    diagnostics:

    error[invalid-fixture]: Discovered an invalid fixture `some_fixture`
     --> test.py:4:5
      |
    4 | def some_fixture() -> int:
      |     ^^^^^^^^^^^^
      |
    info: Invalid fixture scope: sessionss

    error[missing-fixtures]: Test `test_all_scopes` has missing fixtures
     --> test.py:7:5
      |
    7 | def test_all_scopes(some_fixture: int) -> None:
      |     ^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `some_fixture`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
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
            FAIL [TIME] test::test_all_scopes

    diagnostics:

    error[missing-fixtures]: Test `test_all_scopes` has missing fixtures
     --> test.py:2:5
      |
    2 | def test_all_scopes(
      |     ^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `missing_fixture`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

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
            FAIL [TIME] test::test_failing_fixture

    diagnostics:

    error[missing-fixtures]: Test `test_failing_fixture` has missing fixtures
     --> test.py:8:5
      |
    8 | def test_failing_fixture(failing_fixture):
      |     ^^^^^^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `failing_fixture`
    info: Fixture `failing_fixture` failed here
     --> test.py:6:5
      |
    6 |     raise Exception('Fixture failed')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Fixture failed

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fixture_missing_fixtures() {
    let context = TestContext::with_file(
        "test.py",
        r"
                from karva import fixture

                @fixture
                def failing_fixture(missing_fixture):
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
            FAIL [TIME] test::test_failing_fixture

    diagnostics:

    error[missing-fixtures]: Test `test_failing_fixture` has missing fixtures
     --> test.py:8:5
      |
    8 | def test_failing_fixture(failing_fixture):
      |     ^^^^^^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `failing_fixture`
    info: failing_fixture() missing 1 required positional argument: 'missing_fixture'

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

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

    diagnostics:

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
            FAIL [TIME] test::test_failing_fixture

    diagnostics:

    error[missing-fixtures]: Test `test_failing_fixture` has missing fixtures
      --> test.py:10:5
       |
    10 | def test_failing_fixture(fixture):
       |     ^^^^^^^^^^^^^^^^^^^^
       |
    info: Missing fixtures: `fixture`
    info: Fixture `fixture` failed here
     --> test.py:7:9
      |
    7 |         raise ValueError('foo')
      |         ^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: foo

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

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
            PASS [TIME] test::test_fixture_generator(fixture_generator=1)

    diagnostics:

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `fixture_generator`
     --> test.py:5:5
      |
    5 | def fixture_generator():
      |     ^^^^^^^^^^^^^^^^^
      |
    info: Fixture had more than one yield statement

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

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
            PASS [TIME] test::test_fixture_generator(fixture_generator=1)

    diagnostics:

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `fixture_generator`
     --> test.py:5:5
      |
    5 | def fixture_generator():
      |     ^^^^^^^^^^^^^^^^^
      |
    info: Failed to reset fixture: fixture-error

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

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
            FAIL [TIME] test::test_with_db

    diagnostics:

    error[missing-fixtures]: Test `test_with_db` has missing fixtures
      --> test.py:16:5
       |
    16 | def test_with_db(db):
       |     ^^^^^^^^^^^^
       |
    info: Missing fixtures: `db`
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
    info: Fixture `config` failed here
     --> test.py:6:5
      |
    6 |     raise Exception('config failed')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: config failed

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

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
            FAIL [TIME] test::test_with_fixture

    diagnostics:

    error[invalid-fixture]: Discovered an invalid fixture `my_fixture`
     --> test.py:4:5
      |
    4 | def my_fixture():
      |     ^^^^^^^^^^
      |
    info: Scope must be either a string or a callable

    error[missing-fixtures]: Test `test_with_fixture` has missing fixtures
     --> test.py:7:5
      |
    7 | def test_with_fixture(my_fixture):
      |     ^^^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `my_fixture`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}
