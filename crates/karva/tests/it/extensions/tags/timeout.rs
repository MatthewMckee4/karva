use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[test]
fn test_timeout_passes_when_under_limit() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.timeout(5.0)
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
fn test_timeout_fails_when_exceeded_pytest() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time
import pytest

@pytest.mark.timeout(0.1)
def test_slow():
    time.sleep(2)
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

    error[test-failure]: Test `test_slow` failed
     --> test.py:6:5
      |
    6 | def test_slow():
      |     ^^^^^^^^^
    info: Test exceeded timeout of 0.1 seconds
    info: Worker terminated at the deadline; fixture teardown could not be guaranteed.

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_timeout_async_test() {
    let context = TestContext::with_file(
        "test.py",
        r"
import asyncio
import karva

@karva.tags.timeout(0.1)
async def test_slow_async():
    await asyncio.sleep(2)
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

    error[test-failure]: Test `test_slow_async` failed
     --> test.py:6:11
      |
    6 | async def test_slow_async():
      |           ^^^^^^^^^^^^^^^
    info: Test exceeded timeout of 0.1 seconds

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_timeout_with_retry_eventually_passes() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os
import time
import karva

@karva.tags.timeout(0.5)
def test_slow_then_fast():
    if os.environ['KARVA_ATTEMPT'] == '1':
        time.sleep(2)
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=2"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_slow_then_fast
      TRY 2 PASS [TIME] test::test_slow_then_fast
    ────────────
         Summary [TIME] 1 test run: 1 passed (1 flaky), 0 skipped
       FLAKY 2/3 [TIME] test::test_slow_then_fast

    ----- stderr -----
    ");
}

#[rstest]
fn test_timeout_invalid_seconds_rejected(
    #[values("0", "-1", "float('nan')", "float('inf')")] arg: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import karva

@karva.tags.timeout({arg})
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

        error[failed-to-import-module]: Failed to import python module `test`: timeout seconds must be a finite, positive number

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_pytest_timeout_invalid_seconds_rejected(
    #[values("0", "-1", "float('nan')", "float('inf')", "'slow'")] arg: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import pytest

@pytest.mark.timeout({arg})
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

        error[failed-to-import-module]: Failed to import python module `test`: pytest timeout mark seconds must be a finite, positive number

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn test_timeout_with_parametrize_each_case_gets_fresh_window() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time
import karva

@karva.tags.timeout(0.3)
@karva.tags.parametrize('sleep_for', [0.0, 2.0, 0.0])
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
            FAIL [TIME] test::test_1(sleep_for=2.0)
            PASS [TIME] test::test_1(sleep_for=0.0)

    failures:

    test::test_1(sleep_for=2.0):

    error[test-failure]: Test `test_1` failed
     --> test.py:7:5
      |
    7 | def test_1(sleep_for):
      |     ^^^^^^
    info: Test exceeded timeout of 0.3 seconds
    info: Worker terminated at the deadline; fixture teardown could not be guaranteed.

    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_timeout_combined_with_skip_does_not_run() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time
import karva

@karva.tags.timeout(0.1)
@karva.tags.skip(reason='not today')
def test_1():
    time.sleep(2)
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_timeout_with_retry_exhausts_on_always_timing_out() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time
import karva

@karva.tags.timeout(0.1)
def test_always_slow():
    time.sleep(2)
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_always_slow
      TRY 2 FAIL [TIME] test::test_always_slow

    failures:

    test::test_always_slow:

    error[test-failure]: Test `test_always_slow` failed
     --> test.py:6:5
      |
    6 | def test_always_slow():
      |     ^^^^^^^^^^^^^^^^
    info: Test exceeded timeout of 0.1 seconds
    info: Worker terminated at the deadline; fixture teardown could not be guaranteed.

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

/// `--timeout` applies to every test that does not already carry an
/// `@karva.tags.timeout` decorator.
#[test]
fn test_cli_timeout_kills_slow_test() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time

def test_slow():
    time.sleep(2)
        ",
    );

    assert_cmd_snapshot!(context.command().arg("--timeout=0.1"), @"
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
    info: Worker terminated at the deadline; fixture teardown could not be guaranteed.

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_cli_timeout_does_not_flag_fast_tests() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_fast():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command().arg("--timeout=60"), @"
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

/// A test-level `@karva.tags.timeout` overrides the configured default.
#[test]
fn test_cli_timeout_tag_overrides_default() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time
import karva

@karva.tags.timeout(2.0)
def test_under_tag_limit():
    time.sleep(0.3)
        ",
    );

    assert_cmd_snapshot!(context.command().arg("--timeout=0.1"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_under_tag_limit
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_config_slow_timeout_flags_slow_test() {
    let context = TestContext::with_files([
        (
            "pyproject.toml",
            r"
[tool.karva.profile.default.test]
slow-timeout = 0.001
            ",
        ),
        (
            "test.py",
            r"
import time

def test_slow():
    time.sleep(0.05)
            ",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--status-level=slow"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            SLOW [TIME] test::test_slow
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped, 1 slow

    ----- stderr -----
    "
    );
}

#[test]
fn test_config_timeout_kills_slow_test() {
    let context = TestContext::with_files([
        (
            "pyproject.toml",
            r"
[tool.karva.profile.default.test]
timeout = 0.1
            ",
        ),
        (
            "test.py",
            r"
import time

def test_slow():
    time.sleep(2)
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

    error[test-failure]: Test `test_slow` failed
     --> test.py:4:5
      |
    4 | def test_slow():
      |     ^^^^^^^^^
    info: Test exceeded timeout of 0.1 seconds
    info: Worker terminated at the deadline; fixture teardown could not be guaranteed.

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_timeout_preserves_fixture_context_variables() {
    let context = TestContext::with_file(
        "test.py",
        r#"
from contextvars import ContextVar

import karva

request_id = ContextVar("request_id")

@karva.fixture
def request_context():
    token = request_id.set("abc123")
    yield
    request_id.reset(token)

@karva.tags.timeout(60)
def test_context(request_context):
    assert request_id.get() == "abc123"
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_context(request_context=None)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_timeout_with_runtime_skip() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.timeout(60)
def test_skip():
    karva.skip('not today')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_timeout_with_expected_failure() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.timeout(60)
@karva.tags.expect_fail
def test_expected_failure():
    assert False
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_expected_failure
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_timeout_runs_fixture_teardown_before_next_test() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

events = []

@karva.fixture
def resource():
    events.append('setup')
    yield 'resource'
    events.append('teardown')

@karva.tags.timeout(60)
def test_resource(resource):
    assert events == ['setup']

@karva.tags.timeout(60)
def test_after_resource():
    assert events == ['setup', 'teardown']
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_resource(resource='resource')
            PASS [TIME] test::test_after_resource
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_timeout_exposes_retry_environment() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os
import karva

@karva.tags.timeout(60)
def test_retry():
    attempt = int(os.environ['KARVA_ATTEMPT'])
    assert os.environ['KARVA_TOTAL_ATTEMPTS'] == '2'
    assert attempt == 2
        ",
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
fn test_timeout_exposes_parametrized_test_name() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os
import karva

@karva.tags.timeout(60)
@karva.tags.parametrize('value', [1, 2])
def test_name(value):
    assert os.environ['KARVA_TEST_NAME'] == f'test::test_name(value={value})'
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_name(value=1)
            PASS [TIME] test::test_name(value=2)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn hard_timeout_stops_python_and_native_calls_and_preserves_output(
    #[values(
        "while True: pass",
        "time.sleep(3600)",
        "lock = threading.Lock(); lock.acquire(); lock.acquire()"
    )]
    blocking: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import karva
import os
import sys
import time
import threading
from pathlib import Path

@karva.fixture
def resource():
    yield
    Path('teardown.txt').write_text('completed')

@karva.tags.timeout(0.1)
@karva.tags.expect_fail(raises=Exception)
def test_a_blocking(resource):
    print('output before deadline')
    print('stderr before deadline', file=sys.stderr)
    {blocking}

def test_b_remaining():
    assert not Path('teardown.txt').exists()
    print('remaining test ran once')
"
        ),
    );
    allow_duplicates! {{ assert_cmd_snapshot!(context.command_no_parallel()); }}
}

#[test]
fn hard_timeout_retry_uses_fresh_python_state_and_reports_all_attempts() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            "[profile.default.junit]\npath = 'results.xml'\nstore-failure-output = true\n",
        ),
        (
            "test.py",
            r"
import karva
import os
import time
state = []
@karva.tags.timeout(0.1)
def test_timeout():
    assert state == []
    state.append('dirty')
    attempt = os.environ['KARVA_ATTEMPT']
    print(f'attempt {attempt}')
    if attempt == '1':
        while True: pass
",
        ),
    ]);
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--retry=1", "--result-output=results.json"])
    );
    insta::assert_snapshot!(context.read_file("results.json"));
    assert!(context.read_file("results.xml").contains("flakyFailure"));
}

#[test]
fn hard_timeout_preserves_history_when_retry_crashes() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
import os
@karva.tags.timeout(0.1)
def test_timeout():
    if os.environ['KARVA_ATTEMPT'] == '1':
        print('first attempt timed out')
        while True: pass
    os._exit(17)
",
    );
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--retry=1", "--result-output=results.json",])
    );
    insta::assert_snapshot!(context.read_file("results.json"));
}

#[cfg(unix)]
#[test]
fn hard_timeout_stops_native_call_holding_the_gil() {
    let context = TestContext::with_file(
        "test.py",
        r"
import ctypes
import karva
import os

@karva.tags.timeout(0.1)
def test_a_native():
    # Native exit handlers may themselves wait for the GIL held by the test.
    callback_type = ctypes.CFUNCTYPE(None, ctypes.c_void_p)
    exit_callback = callback_type(lambda argument: None)
    native = ctypes.CDLL(None)
    native.__cxa_atexit.argtypes = [callback_type, ctypes.c_void_p, ctypes.c_void_p]
    native.__cxa_atexit(exit_callback, None, None)
    print('before native call')
    os.write(2, b'native stderr before deadline\n')
    ctypes.PyDLL(None).sleep(3600)

def test_b_remaining():
    pass
",
    );
    assert_cmd_snapshot!(context.command_no_parallel());
}
