use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_hypothesis_given_with_async_test() {
    let context = TestContext::with_file(
        "test.py",
        r"
from hypothesis import given
from hypothesis import strategies as st

@given(x=st.integers(min_value=0, max_value=10))
async def test_async_with_given(x):
    assert isinstance(x, int)
    assert 0 <= x <= 10
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_async_with_given
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_async_function() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio

async def test_async_passes():
    await asyncio.sleep(0)
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_async_passes
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_async_function_with_assertion_error() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio

async def test_async_fails():
    await asyncio.sleep(0)
    assert False, 'async test failed'
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_async_fails

    failures:

    test::test_async_fails:

    error[test-failure]: Test `test_async_fails` failed
     --> test.py:4:11
      |
    4 | async def test_async_fails():
      |           ^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test.py:6:5
      |
    6 |     assert False, 'async test failed'
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: async test failed

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_async_fixture() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio
import karva

@karva.fixture
async def async_value():
    await asyncio.sleep(0)
    return 42

async def test_with_async_fixture(async_value):
    assert async_value == 42
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_with_async_fixture(async_value=42)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_async_generator_fixture() {
    let context = TestContext::with_files([(
        "test.py",
        r"
import asyncio
import karva

setup_done = False
teardown_done = False

@karva.fixture
async def async_resource():
    global setup_done
    setup_done = True
    await asyncio.sleep(0)
    yield 'resource'
    global teardown_done
    teardown_done = True

async def test_async_gen_fixture(async_resource):
    assert async_resource == 'resource'
    assert setup_done is True

def test_teardown_ran():
    assert teardown_done is True
        ",
    )]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_async_gen_fixture(async_resource='resource')
            PASS [TIME] test::test_teardown_ran
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_async_test_with_sync_fixture() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio
import karva

@karva.fixture
def sync_value():
    return 10

async def test_async_with_sync(sync_value):
    await asyncio.sleep(0)
    assert sync_value == 10
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_async_with_sync(sync_value=10)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_sync_test_with_async_fixture() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio
import karva

@karva.fixture
async def async_value():
    await asyncio.sleep(0)
    return 99

def test_sync_with_async(async_value):
    assert async_value == 99
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_sync_with_async(async_value=99)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}
