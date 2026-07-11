use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

/// A run that exceeds `--run-timeout` is stopped and reported as a failure.
///
/// The test sleeps far longer than the one-second limit, so the timeout fires
/// deterministically before it can finish.
#[test]
fn test_run_timeout_stops_long_run() {
    let context = TestContext::with_file(
        "test.py",
        r"
import time

def test_slow():
    time.sleep(30)
        ",
    );

    assert_cmd_snapshot!(context.command().arg("--run-timeout").arg("1"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    error: run timed out before all tests completed

    ----- stderr -----
    ");
}

/// `run-timeout` is also honored when set in configuration.
#[test]
fn test_run_timeout_from_config() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r"
[profile.default.test]
run-timeout = 1.0
",
        ),
        (
            "test.py",
            r"
import time

def test_slow():
    time.sleep(30)
        ",
        ),
    ]);

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    error: run timed out before all tests completed

    ----- stderr -----
    ");
}

/// A timed-out run sends SIGTERM before force-killing the worker, giving the
/// running test process a chance to clean up.
#[cfg(unix)]
#[test]
fn test_run_timeout_sends_sigterm_before_force_kill() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os
from pathlib import Path
import signal
import time

def handle_sigterm(signum, frame):
    Path('terminated').write_text('1')
    os._exit(0)

signal.signal(signal.SIGTERM, handle_sigterm)

def test_slow():
    time.sleep(30)
        ",
    );

    assert_cmd_snapshot!(
        context
            .command()
            .arg("--run-timeout=1")
            .arg("--termination-grace-period=2"),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    error: run timed out before all tests completed

    ----- stderr -----
    "
    );

    assert!(context.root().join("terminated").exists());
}

/// A run that finishes within `--run-timeout` is unaffected.
#[test]
fn test_run_timeout_allows_fast_run() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_fast():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command().arg("--run-timeout").arg("600"), @"
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
