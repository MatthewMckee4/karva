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
      |
    info: Test failed here
     --> test.py:6:5
      |
    6 |     assert False, 'async test failed'
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
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

/// A background task that fails without anything awaiting it cannot reach the
/// test coroutine, so the loop reports it instead. The test must not pass.
#[test]
fn test_unhandled_background_task_fails_test() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio

async def fail_in_background():
    raise RuntimeError('lost failure')

async def test_background_work():
    asyncio.create_task(fail_in_background())
    await asyncio.sleep(0)
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_background_work

    failures:

    test::test_background_work:

    error[test-failure]: Test `test_background_work` failed
     --> test.py:7:11
      |
    7 | async def test_background_work():
      |           ^^^^^^^^^^^^^^^^^^^^
      |
    info: Unhandled exception in background task: Task-[N]: RuntimeError: lost failure

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// Awaiting the task and handling its exception is ordinary control flow and
/// must not be reported as a background failure.
#[test]
fn test_awaited_background_failure_still_passes() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio

async def boom():
    raise RuntimeError('handled')

async def test_awaited():
    task = asyncio.create_task(boom())
    try:
        await task
    except RuntimeError:
        pass

async def test_exception_retrieved():
    task = asyncio.create_task(boom())
    await asyncio.sleep(0)
    assert task.exception() is not None

async def test_gather_return_exceptions():
    results = await asyncio.gather(boom(), return_exceptions=True)
    assert isinstance(results[0], RuntimeError)
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_awaited
            PASS [TIME] test::test_exception_retrieved
            PASS [TIME] test::test_gather_return_exceptions
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

/// `asyncio.run` cancels tasks that are still pending when the test coroutine
/// returns. That shutdown is clean and must not fail the test.
#[test]
fn test_pending_task_cancelled_at_shutdown_still_passes() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio

async def test_leaves_pending_task():
    asyncio.create_task(asyncio.sleep(3600))
    await asyncio.sleep(0)
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_leaves_pending_task
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

/// Every background failure is retained rather than only the first.
#[test]
fn test_multiple_background_failures_are_all_reported() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio

async def boom(message):
    raise RuntimeError(message)

async def test_two_failures():
    asyncio.create_task(boom('first'))
    asyncio.create_task(boom('second'))
    await asyncio.sleep(0)
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_two_failures

    failures:

    test::test_two_failures:

    error[test-failure]: Test `test_two_failures` failed
     --> test.py:7:11
      |
    7 | async def test_two_failures():
      |           ^^^^^^^^^^^^^^^^^
      |
    info: 2 unhandled exceptions in background tasks:
            [1] Task-[N]: RuntimeError: first
            [2] Task-[N]: RuntimeError: second

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// The timeout path drives the coroutine through a separate `asyncio.run`, so
/// it needs the same watch on the loop.
#[test]
fn test_background_failure_under_timeout_tag() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio
import karva

async def boom():
    raise RuntimeError('under timeout')

@karva.tags.timeout(5.0)
async def test_with_timeout():
    asyncio.create_task(boom())
    await asyncio.sleep(0)
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_with_timeout

    failures:

    test::test_with_timeout:

    error[test-failure]: Test `test_with_timeout` failed
     --> test.py:9:11
      |
    9 | async def test_with_timeout():
      |           ^^^^^^^^^^^^^^^^^
      |
    info: Unhandled exception in background task: Task-[N]: RuntimeError: under timeout

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// Async fixture setup runs on its own loop, so a failure started there is
/// attributed to the fixture rather than to the test body.
#[test]
fn test_background_failure_in_async_fixture() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio
import karva

async def boom():
    raise RuntimeError('fixture background')

@karva.fixture
async def leaky():
    asyncio.create_task(boom())
    await asyncio.sleep(0)
    yield 'value'

async def test_uses_leaky_fixture(leaky):
    assert leaky == 'value'
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_uses_leaky_fixture

    failures:

    test::test_uses_leaky_fixture (requires fixture `leaky`):

    error[fixture-failure]: Fixture `leaky` failed
     --> test.py:9:11
      |
    9 | async def leaky():
      |           ^^^^^
      |
    info: Unhandled exception in background task: Task-[N]: RuntimeError: fixture background

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ");
}

/// A background failure is an ordinary failure, so the configured retry budget
/// applies and a later clean attempt makes the test flaky.
#[test]
fn test_background_failure_is_retryable() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio
import os

async def boom():
    raise RuntimeError('flaky background')

async def test_flaky_background():
    if os.environ['KARVA_ATTEMPT'] == '1':
        asyncio.create_task(boom())
    await asyncio.sleep(0)
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_flaky_background
      TRY 2 PASS [TIME] test::test_flaky_background
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_flaky_background

    ----- stderr -----
    ");
}

/// Each parameter case gets its own event loop, so a failure in one case must
/// not be attributed to the next.
#[test]
fn test_background_failure_isolated_between_parameter_cases() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio
import karva

async def boom():
    raise RuntimeError('only for 1')

@karva.tags.parametrize('value', [1, 2])
async def test_isolated(value):
    if value == 1:
        asyncio.create_task(boom())
    await asyncio.sleep(0)
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_isolated(value=1)
            PASS [TIME] test::test_isolated(value=2)

    failures:

    test::test_isolated(value=1):

    error[test-failure]: Test `test_isolated` failed
     --> test.py:9:11
      |
    9 | async def test_isolated(value):
      |           ^^^^^^^^^^^^^
      |
    info: Test ran with arguments:
    info: `value`: `1`
    info: Unhandled exception in background task: Task-[N]: RuntimeError: only for 1

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}
