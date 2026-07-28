use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[test]
fn test_fail_slow_passes_when_under_budget() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.fail_slow(5.0)
def test_fast():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_fast
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fail_slow_fails_when_exceeded() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time
import karva

@karva.tags.fail_slow(0.1)
def test_slow():
    time.sleep(0.3)
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_slow

    failures:

    test::test_slow:

    error[fail-slow-exceeded]: Test `test_slow` exceeded its fail-slow budget
     --> test.py:6:5
      |
    6 | def test_slow():
      |     ^^^^^^^^^
      |
    info: Configured budget: [TIME], actual duration: [TIME] (slowest phase: call)

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fail_slow_async_test() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio
import karva

@karva.tags.fail_slow(0.1)
async def test_slow_async():
    await asyncio.sleep(0.3)
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_slow_async

    failures:

    test::test_slow_async:

    error[fail-slow-exceeded]: Test `test_slow_async` exceeded its fail-slow budget
     --> test.py:6:11
      |
    6 | async def test_slow_async():
      |           ^^^^^^^^^^^^^^^
      |
    info: Configured budget: [TIME], actual duration: [TIME] (slowest phase: call)

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// A test's teardown always runs before the budget is checked — a
/// budget-exceeded failure never skips cleanup.
#[test]
fn test_fail_slow_runs_teardown_before_failing() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time
import karva

events = []

@karva.fixture
def resource():
    events.append('setup')
    yield 'resource'
    events.append('teardown')

@karva.tags.fail_slow(0.1)
def test_slow(resource):
    time.sleep(0.3)

def test_after():
    assert events == ['setup', 'teardown']
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            FAIL [TIME] test::test_slow(resource=resource)
            PASS [TIME] test::test_after

    failures:

    test::test_slow(resource=resource):

    error[fail-slow-exceeded]: Test `test_slow` exceeded its fail-slow budget
      --> test.py:14:5
       |
    14 | def test_slow(resource):
       |     ^^^^^^^^^
       |
    info: Configured budget: [TIME], actual duration: [TIME] (slowest phase: call)

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// A test that already fails for another reason and also exceeds its
/// budget shows both: the original failure stays primary, the exceeded
/// budget is noted alongside it.
#[test]
fn test_fail_slow_combined_with_assertion_failure_shows_both() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time
import karva

@karva.tags.fail_slow(0.1)
def test_slow_and_wrong():
    time.sleep(0.3)
    assert False
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_slow_and_wrong

    failures:

    test::test_slow_and_wrong:

    error[test-failure]: Test `test_slow_and_wrong` failed
     --> test.py:6:5
      |
    6 | def test_slow_and_wrong():
      |     ^^^^^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:8:5
      |
    8 |     assert False
      |     ^^^^^^^^^^^^
      |

    error[fail-slow-exceeded]: Test `test_slow_and_wrong` exceeded its fail-slow budget
     --> test.py:6:5
      |
    6 | def test_slow_and_wrong():
      |     ^^^^^^^^^^^^^^^^^^^
      |
    info: Configured budget: [TIME], actual duration: [TIME] (slowest phase: call)

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// The budget is only known after the full lifecycle (including teardown)
/// completes, so a slow-but-passing attempt is never retried purely for
/// slowness — it runs once and is reported as an ordinary failure.
#[test]
fn test_fail_slow_is_not_retried_even_with_retry_configured() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time
import karva

attempts = [0]

@karva.tags.fail_slow(0.1)
def test_slow():
    attempts[0] += 1
    time.sleep(0.3)
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=2"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_slow

    failures:

    test::test_slow:

    error[fail-slow-exceeded]: Test `test_slow` exceeded its fail-slow budget
     --> test.py:8:5
      |
    8 | def test_slow():
      |     ^^^^^^^^^
      |
    info: Configured budget: [TIME], actual duration: [TIME] (slowest phase: call)

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn test_fail_slow_invalid_seconds_rejected(
    #[values("0", "-1", "float('nan')", "float('inf')")] arg: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import karva

@karva.tags.fail_slow({arg})
def test_1():
    assert True
        "
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
        diagnostics:

        error[failed-to-import-module]: Failed to import python module `test`: fail_slow seconds must be a finite, positive number

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn test_fail_slow_with_parametrize_only_slow_case_fails() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time
import karva

@karva.tags.fail_slow(0.1)
@karva.tags.parametrize('sleep_for', [0.0, 0.3, 0.0])
def test_1(sleep_for):
    time.sleep(sleep_for)
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_1(sleep_for=0.0)
            FAIL [TIME] test::test_1(sleep_for=0.3)
            PASS [TIME] test::test_1(sleep_for=0.0)

    failures:

    test::test_1(sleep_for=0.3):

    error[fail-slow-exceeded]: Test `test_1` exceeded its fail-slow budget
     --> test.py:7:5
      |
    7 | def test_1(sleep_for):
      |     ^^^^^^
      |
    info: Configured budget: [TIME], actual duration: [TIME] (slowest phase: call)

    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// `--fail-slow` applies to every test that does not already carry an
/// `@karva.tags.fail_slow` decorator.
#[test]
fn test_cli_fail_slow_fails_slow_test() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time

def test_slow():
    time.sleep(0.3)
        ",
    );

    assert_cmd_snapshot!(context.command().arg("--fail-slow=0.1"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_slow

    failures:

    test::test_slow:

    error[fail-slow-exceeded]: Test `test_slow` exceeded its fail-slow budget
     --> test.py:4:5
      |
    4 | def test_slow():
      |     ^^^^^^^^^
      |
    info: Configured budget: [TIME], actual duration: [TIME] (slowest phase: call)

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_cli_fail_slow_does_not_flag_fast_tests() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_fast():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command().arg("--fail-slow=60"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_fast
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

/// A test-level `@karva.tags.fail_slow` overrides the configured default.
#[test]
fn test_cli_fail_slow_tag_overrides_default() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time
import karva

@karva.tags.fail_slow(2.0)
def test_under_tag_budget():
    time.sleep(0.1)
        ",
    );

    assert_cmd_snapshot!(context.command().arg("--fail-slow=0.05"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_under_tag_budget
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_config_fail_slow_fails_slow_test() {
    let context = TestContext::with_files([
        (
            "pyproject.toml",
            r"
[tool.karva.profile.default.test]
fail-slow = 0.1
            ",
        ),
        (
            "test.py",
            r"
import time

def test_slow():
    time.sleep(0.3)
            ",
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_slow

    failures:

    test::test_slow:

    error[fail-slow-exceeded]: Test `test_slow` exceeded its fail-slow budget
     --> test.py:4:5
      |
    4 | def test_slow():
      |     ^^^^^^^^^
      |
    info: Configured budget: [TIME], actual duration: [TIME] (slowest phase: call)

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// A hard `timeout` shorter than the `fail-slow` budget kills the test
/// well within that budget, so no fail-slow diagnostic is produced.
#[test]
fn test_timeout_shorter_than_fail_slow_budget_only_reports_timeout() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time

def test_slow():
    time.sleep(2)
        ",
    );

    assert_cmd_snapshot!(
        context.command().arg("--timeout=0.1").arg("--fail-slow=10.0"),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_slow

    failures:

    test::test_slow:

    error[test-failure]: Test `test_slow` failed
     --> test.py:4:5
      |
    4 | def test_slow():
      |     ^^^^^^^^^
      |
    info: Test exceeded timeout of 0.1 seconds

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );
}
