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
            PASS [TIME] test::test_resource(async_resource=resource)
            PASS [TIME] test::test_after_cleanup(async_resource=resource)
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
      |
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
      |
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
