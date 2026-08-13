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
            FAIL [TIME] test::test_slow(resource='resource')
            PASS [TIME] test::test_after

    failures:

    test::test_slow(resource='resource'):

    error[fail-slow-exceeded]: Test `test_slow` exceeded its fail-slow budget
      --> test.py:14:5
       |
    14 | def test_slow(resource):
       |     ^^^^^^^^^
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
    info: Test failed here
     --> test.py:8:5
      |
    8 |     assert False
      |     ^^^^^^^^^^^^

    error[fail-slow-exceeded]: Test `test_slow_and_wrong` exceeded its fail-slow budget
     --> test.py:6:5
      |
    6 | def test_slow_and_wrong():
      |     ^^^^^^^^^^^^^^^^^^^
    info: Configured budget: [TIME], actual duration: [TIME] (slowest phase: call)

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// A fail-slow failure consumes the retry budget like any other failure.
#[test]
fn test_fail_slow_retries_to_a_fast_attempt() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time
import karva

attempts = [0]

@karva.tags.fail_slow(0.1)
def test_slow():
    attempts[0] += 1
    if attempts[0] == 1:
        time.sleep(0.3)
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_slow
      TRY 2 PASS [TIME] test::test_slow
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_slow

    ----- stderr -----
    ");
}

#[rstest]
fn test_fail_slow_invalid_seconds_rejected(
    #[values("0", "-1", "float('nan')", "float('inf')", "1e-300", "1e300")] arg: &str,
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

        error[failed-to-import-module]: Failed to import python module `test`: fail_slow seconds must be a finite, positive duration supported by this platform

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn test_fail_slow_does_not_sum_retry_attempts() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import time
import karva

# A 0.15s sleep exceeded a 0.2s budget on macOS CI. Keep enough headroom for
# one attempt while two 1s attempts still exceed the per-attempt budget.
@karva.tags.fail_slow(1.5)
def test_retry():
    time.sleep(1)
    assert os.environ["KARVA_ATTEMPT"] == "2"
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_retry
      TRY 2 PASS [TIME] test::test_retry
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry

    ----- stderr -----
    ");
}

#[test]
fn test_fail_slow_sets_attempt_environment_before_retry_fixture_setup() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import time
import karva

fixture_attempts = []

@karva.fixture
def resource():
    fixture_attempts.append(os.environ.get("KARVA_ATTEMPT"))
    yield
    if len(fixture_attempts) == 1:
        time.sleep(0.3)

@karva.tags.fail_slow(0.2)
def test_retry(resource):
    if os.environ["KARVA_ATTEMPT"] == "2":
        assert fixture_attempts[-1] == "2"
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_retry(resource=None)
      TRY 2 PASS [TIME] test::test_retry(resource=None)
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/2 [TIME] test::test_retry(resource=None)

    ----- stderr -----
    ");
}

#[test]
fn test_fail_slow_setup_error_reports_slow_teardown() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import time
import karva

@karva.fixture
def established():
    yield
    time.sleep(0.4)

@karva.fixture
def broken(established):
    raise RuntimeError("setup failed")

@karva.tags.fail_slow(0.2)
def test_example(broken):
    pass
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           ERROR [TIME] test::test_example

    failures:

    test::test_example (requires fixture `broken`):

    error[fixture-failure]: Fixture `broken` failed
      --> test.py:11:5
       |
    11 | def broken(established):
       |     ^^^^^^
    info: Fixture ran with arguments:
      info: `established`: `None`
    info: Fixture failed here
      --> test.py:12:5
       |
    12 |     raise RuntimeError("setup failed")
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: setup failed

    error[fail-slow-exceeded]: Test `test_example` exceeded its fail-slow budget
      --> test.py:15:5
       |
    15 | def test_example(broken):
       |     ^^^^^^^^^^^^
    info: Configured budget: [TIME], actual duration: [TIME] (slowest phase: teardown)

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn test_fail_slow_excludes_test_name_rendering() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import time
import karva

class SlowString:
    def __str__(self):
        time.sleep(0.3)
        return "value"

@karva.fixture
def value():
    return SlowString()

@karva.tags.fail_slow(0.2)
def test_example(value):
    pass
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_example(value=value)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
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
    info: Test exceeded timeout of 0.1 seconds

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );
}
