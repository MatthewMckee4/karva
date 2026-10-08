use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[test]
fn test_fixture_generator() {
    let test_context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.fixture
def fixture_generator():
    yield 1

def test_fixture_generator(fixture_generator):
    assert fixture_generator == 1
",
    );

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_fixture_generator(fixture_generator=1)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_async_generator_fixture() {
    let test_context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.fixture
async def async_fixture():
    yield 42

async def test_async_fixture(async_fixture):
    assert async_fixture == 42
",
    );

    assert_cmd_snapshot!(test_context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_async_fixture(async_fixture=42)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_async_generator_fixture_with_teardown() {
    let test_context = TestContext::with_file(
        "test.py",
        r"
import karva

arr = []

@karva.fixture
async def async_resource():
    yield 'resource'
    arr.append('cleaned')

async def test_resource(async_resource):
    assert async_resource == 'resource'
    assert len(arr) == 0

async def test_after_cleanup(async_resource):
    assert len(arr) == 1
",
    );

    assert_cmd_snapshot!(test_context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_resource(async_resource='resource')
            PASS [TIME] test::test_after_cleanup(async_resource='resource')
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_async_generator_fixture_multiple_yields() {
    let test_context = TestContext::with_file(
        "test.py",
        r"import karva

@karva.fixture
async def bad_fixture():
    yield 1
    yield 2

async def test_bad(bad_fixture):
    assert bad_fixture == 1
",
    );

    assert_cmd_snapshot!(test_context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_bad(bad_fixture=1)

    failures:

    test::test_bad(bad_fixture=1):

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `bad_fixture`
     --> test.py:4:11
      |
    4 | async def bad_fixture():
      |           ^^^^^^^^^^^
    info: Fixture had more than one yield statement

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_async_generator_fixture_error_in_teardown() {
    let test_context = TestContext::with_file(
        "test.py",
        r#"import karva

@karva.fixture
async def error_fixture():
    yield 1
    raise RuntimeError("teardown failed")

async def test_error(error_fixture):
    assert error_fixture == 1
"#,
    );

    assert_cmd_snapshot!(test_context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_error(error_fixture=1)

    failures:

    test::test_error(error_fixture=1):

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `error_fixture`
     --> test.py:4:11
      |
    4 | async def error_fixture():
      |           ^^^^^^^^^^^^^
    info: Failed to reset fixture: teardown failed

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn test_fixture_generator_with_second_fixture(#[values("karva", "pytest")] framework: &str) {
    let test_context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(test_context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_fixture_generator(fixture_generator=1)
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_fixture_generator_finalizer_order(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
                import {framework}

                execution_log = []

                @{framework}.fixture
                def ordered_fixture():
                    execution_log.append('setup')
                    yield 'value'
                    execution_log.append('teardown')

                def test_one(ordered_fixture):
                    execution_log.append('test_one')
                    assert ordered_fixture == 'value'

                def test_check_order():
                    # After test_one completes, fixture is torn down
                    assert execution_log == [
                        'setup',
                        'test_one',
                        'teardown',
                    ], execution_log
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] test::test_one(ordered_fixture='value')
                PASS [TIME] test::test_check_order
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_independent_generator_fixtures_finalize_in_reverse_order(
    #[values("pytest", "karva")] framework: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"

            import {framework}

            execution_log: list[str] = []


            @{framework}.fixture
            def ordered_fixture():
                execution_log.append("1_setup")
                yield "value1"
                execution_log.append("1_teardown")


            @{framework}.fixture
            def ordered_fixture2():
                execution_log.append("2_setup")
                yield "value2"
                execution_log.append("2_teardown")


            def test_one(ordered_fixture, ordered_fixture2):
                execution_log.append("test")


            def test_check_order():
                # After test_one, both fixtures are torn down in reverse order
                assert execution_log == [
                    "1_setup",
                    "2_setup",
                    "test",
                    "2_teardown",
                    "1_teardown",
                ], execution_log

"#
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 2 tests across 1 worker
                PASS [TIME] test::test_one(ordered_fixture='value1', ordered_fixture2='value2')
                PASS [TIME] test::test_check_order
        ────────────
             Summary [TIME] 2 tests run: 2 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_shared_generator_dependency_finalizes_once(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
from pathlib import Path
import {framework}

LOG = Path("lifecycle.log")
LOG.write_text("")

@{framework}.fixture
def shared():
    with LOG.open("a") as stream:
        stream.write("setup\n")
    yield []
    with Path("lifecycle.log").open("a") as stream:
        stream.write("teardown\n")

@{framework}.fixture
def left(shared):
    return shared

@{framework}.fixture
def right(shared):
    return shared

def test_shared(left, right, shared):
    assert left is right is shared
    assert Path("lifecycle.log").read_text() == "setup\n"
"#
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_shared(left=[], right=[], shared=[])
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
    assert_eq!(
        context
            .read_file("lifecycle.log")
            .lines()
            .collect::<Vec<_>>(),
        ["setup", "teardown"]
    );
}

#[rstest]
fn test_generator_dependency_finalizes_after_setup_failure(
    #[values("pytest", "karva")] framework: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
from pathlib import Path
import {framework}

@{framework}.fixture
def resource():
    yield "resource"
    Path("cleaned-up").touch()

@{framework}.fixture
def broken(resource):
    raise RuntimeError("setup failed")

def test_resource(broken):
    Path("test-ran").touch()
"#
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test::test_resource

        failures:

        test::test_resource (requires fixture `broken`):

        error[fixture-failure]: Fixture `broken` failed
          --> test.py:11:5
           |
        11 | def broken(resource):
           |     ^^^^^^
        info: Fixture ran with arguments:
        info:   `resource`: `resource`
        info: Fixture failed here
          --> test.py:12:5
           |
        12 |     raise RuntimeError("setup failed")
           |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: setup failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
    }
    assert!(context.root().join("cleaned-up").exists());
    assert!(!context.root().join("test-ran").exists());
}

#[rstest]
fn test_generator_finalizes_after_test_failure(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
from pathlib import Path
import {framework}

@{framework}.fixture
def resource():
    yield "resource"
    Path("cleaned-up").touch()

def test_resource(resource):
    assert False, "test failed"
"#
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
                FAIL [TIME] test::test_resource(resource='resource')

        failures:

        test::test_resource(resource='resource'):

        error[test-failure]: Test `test_resource` failed
          --> test.py:10:5
           |
        10 | def test_resource(resource):
           |     ^^^^^^^^^^^^^
        info: Test ran with arguments:
        info:   `resource`: `resource`
        info: Test failed here
          --> test.py:11:5
           |
        11 |     assert False, "test failed"
           |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: test failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

        ----- stderr -----
        "#);
    }
    assert!(context.root().join("cleaned-up").exists());
}

#[rstest]
fn test_generator_dependency_finalizes_despite_teardown_failure(
    #[values("pytest", "karva")] framework: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r#"
from pathlib import Path
import {framework}

@{framework}.fixture
def resource():
    yield "resource"
    Path("cleaned-up").touch()

@{framework}.fixture
def broken(resource):
    yield resource
    raise RuntimeError("teardown failed")

def test_resource(broken):
    assert broken == "resource"
"#
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test::test_resource(broken='resource')

        failures:

        test::test_resource(broken='resource'):

        error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `broken`
          --> test.py:11:5
           |
        11 | def broken(resource):
           |     ^^^^^^
        info: Failed to reset fixture: teardown failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        ");
    }
    assert!(context.root().join("cleaned-up").exists());
}
