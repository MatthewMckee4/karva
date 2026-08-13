use insta_cmd::assert_cmd_snapshot;

#[cfg(unix)]
use insta::assert_snapshot;
#[cfg(unix)]
use std::process::Stdio;
#[cfg(unix)]
use std::time::{Duration, Instant};

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

/// A descendant outside the worker process group cannot retain the controller
/// stream and stall timeout cleanup.
#[cfg(unix)]
#[test]
fn test_run_timeout_closes_stream_held_by_escaped_descendant() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os
from pathlib import Path
import time

def test_slow():
    ready_read, ready_write = os.pipe()
    if os.fork() == 0:
        os.close(ready_read)
        os.setsid()
        Path('escaped_pid').write_text(str(os.getpid()))
        os.write(ready_write, b'1')
        time.sleep(30)
        os._exit(0)
    os.close(ready_write)
    os.read(ready_read, 1)
    time.sleep(30)
",
    );
    let mut child = context
        .command()
        .args(["--run-timeout=1", "--termination-grace-period=0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn Karva");

    // Receipt: the configured one-second timeout gets four additional seconds
    // for loaded CI scheduling. The escaped child sleeps for 30 seconds, so a
    // retained controller stream would exceed this tripwire deterministically.
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll Karva") {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill stalled Karva");
            if let Ok(pid) =
                std::fs::read_to_string(context.root().join("escaped_pid")).and_then(|pid| {
                    pid.parse::<u32>()
                        .map_err(|error| std::io::Error::other(error.to_string()))
                })
            {
                std::process::Command::new("kill")
                    .args(["-KILL", &pid.to_string()])
                    .status()
                    .expect("kill escaped descendant");
            }
            panic!("Karva did not close the escaped descendant's controller stream");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = child.wait_with_output().expect("collect Karva output");
    if let Ok(pid) = std::fs::read_to_string(context.root().join("escaped_pid")).and_then(|pid| {
        pid.parse::<u32>()
            .map_err(|error| std::io::Error::other(error.to_string()))
    }) {
        std::process::Command::new("kill")
            .args(["-KILL", &pid.to_string()])
            .status()
            .expect("kill escaped descendant");
    }

    assert_snapshot!(format!(
        "success: {}\nexit_code: {}\n----- stdout -----\n{}\n----- stderr -----\n{}",
        status.success(),
        status.code().map_or_else(|| "none".to_string(), |code| code.to_string()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    ), @"
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
