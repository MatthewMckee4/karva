use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use insta::assert_snapshot;
use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

mod uncertain;

#[test]
fn worker_exit_kills_descendants_holding_controller_streams() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os
import signal


def test_crash_with_child():
    if os.fork() == 0:
        signal.pause()
    os._exit(28)
",
    );

    assert_cmd_snapshot!(context.command(), @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           CRASH [TIME] test::test_crash_with_child

    failures:

    test::test_crash_with_child:

    error[worker-crashed]: Worker terminated with exit code 28 while running `test::test_crash_with_child`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 28 in [TIME]
    "###);
}

#[test]
fn worker_exit_kills_descendants_after_controller_disconnect() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path
import signal
import time


def test_crash_with_disconnected_child():
    if os.fork() == 0:
        os.closerange(3, os.sysconf("SC_OPEN_MAX"))
        signal.alarm(30)
        Path("descendant_pid").write_text(str(os.getpid()))
        signal.pause()
    while not Path("descendant_pid").exists():
        time.sleep(0.001)
    os._exit(32)
"#,
    );
    let output = run_with_descendant(&context);
    assert_descendant_stopped(&context);

    assert_snapshot!(snapshot_output(&output), @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           CRASH [TIME] test::test_crash_with_disconnected_child

    failures:

    test::test_crash_with_disconnected_child:

    error[worker-crashed]: Worker terminated with exit code 32 while running `test::test_crash_with_disconnected_child`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 32 in [TIME]
    "###);
}

#[test]
fn worker_exit_does_not_wait_for_disconnected_escaped_descendant_output() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path
import signal
import time


def test_crash_with_disconnected_escaped_child():
    if os.fork() == 0:
        os.setsid()
        os.closerange(3, os.sysconf("SC_OPEN_MAX"))
        signal.alarm(30)
        Path("descendant_pid").write_text(str(os.getpid()))
        signal.pause()
    while not Path("descendant_pid").exists():
        time.sleep(0.001)
    os._exit(34)
"#,
    );
    let output = run_with_descendant(&context);
    terminate_escaped_descendant(&context);

    assert_snapshot!(snapshot_output(&output), @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           CRASH [TIME] test::test_crash_with_disconnected_escaped_child

    failures:

    test::test_crash_with_disconnected_escaped_child:

    error[worker-crashed]: Worker terminated with exit code 34 while running `test::test_crash_with_disconnected_escaped_child`

    Worker stderr:
    [Karva stopped draining worker output after the 50 ms limit; final output and results may be incomplete]

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    WARN worker output drain limit reached; final output and results may be incomplete worker_id=0 limit_ms=50
    ERROR Worker 0 failed with exit code 34 in [TIME]
    "###);
}

#[test]
fn completed_worker_does_not_wait_for_escaped_descendant() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path
import signal
import time


def test_pass_with_escaped_child():
    if os.fork() == 0:
        os.setsid()
        signal.alarm(30)
        Path("descendant_pid").write_text(str(os.getpid()))
        signal.pause()
    while not Path("descendant_pid").exists():
        time.sleep(0.001)
"#,
    );
    let output = run_with_descendant(&context);
    terminate_escaped_descendant(&context);

    assert_snapshot!(snapshot_output(&output), @r###"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    WARN worker output drain limit reached; final output may be incomplete worker_id=0 limit_ms=50
    "###);
}

fn run_with_descendant(context: &TestContext) -> Output {
    let mut karva = context
        .command()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn Karva");

    // Receipt: normal cleanup completes within five 10 ms supervisor polls.
    // Five seconds allows 100 times that observation on loaded CI; each test
    // descendant has a 30-second alarm as a final leak fallback.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if karva.try_wait().expect("poll Karva").is_some() {
            return karva.wait_with_output().expect("collect Karva output");
        }
        if Instant::now() >= deadline {
            karva.kill().expect("kill stalled Karva");
            let descendant_cleanup = kill_descendant_if_started(context);
            let output = karva.wait_with_output().expect("collect Karva output");
            panic!(
                "Karva stalled while cleaning up a worker descendant (descendant cleanup: {descendant_cleanup:?})\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn snapshot_output(output: &Output) -> String {
    format!(
        "success: {}\nexit_code: {}\n----- stdout -----\n{}\n----- stderr -----\n{}",
        output.status.success(),
        output
            .status
            .code()
            .map_or_else(|| "none".to_string(), |code| code.to_string()),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

fn assert_descendant_stopped(context: &TestContext) {
    let pid = context.read_file("descendant_pid");
    let state = process_state(pid.trim());
    if state
        .as_deref()
        .is_some_and(|state| !state.starts_with('Z'))
    {
        let cleanup_succeeded = kill_process(pid.trim());
        panic!(
            "worker descendant {pid} remained alive in state {state:?} (cleanup succeeded: {cleanup_succeeded})"
        );
    }
}

fn terminate_escaped_descendant(context: &TestContext) {
    let pid = context.read_file("descendant_pid");
    let state = process_state(pid.trim());
    assert!(
        state
            .as_deref()
            .is_some_and(|state| !state.starts_with('Z')),
        "escaped worker descendant {pid} was not alive after Karva exited: {state:?}"
    );
    assert!(
        kill_process(pid.trim()),
        "failed to kill escaped worker descendant {pid}"
    );
}

fn kill_descendant_if_started(context: &TestContext) -> Option<bool> {
    std::fs::read_to_string(context.root().join("descendant_pid"))
        .ok()
        .map(|pid| kill_process(pid.trim()))
}

fn kill_process(pid: &str) -> bool {
    Command::new("kill")
        .args(["-KILL", pid])
        .status()
        .is_ok_and(|status| status.success())
}

fn process_state(pid: &str) -> Option<String> {
    let output = Command::new("ps")
        .args(["-o", "state=", "-p", pid])
        .output()
        .expect("inspect worker descendant");
    if !output.status.success() {
        return None;
    }
    let state = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!state.is_empty()).then_some(state)
}
