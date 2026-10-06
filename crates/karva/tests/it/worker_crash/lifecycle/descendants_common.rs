use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use insta::assert_snapshot;

use crate::common::TestContext;

#[test]
fn completed_worker_kills_portable_grandchild() -> Result<(), String> {
    let context = descendant_context(false);
    let output = run_karva(&context, &[])?;

    assert_snapshot!(snapshot_output(&output), @r###"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_starts_child
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "###);
    assert_descendants_stopped(&context);
    Ok(())
}

#[test]
fn timed_out_worker_kills_portable_grandchild() -> Result<(), String> {
    let context = descendant_context(true);
    let output = run_karva_when_ready(
        &context,
        &["--run-timeout=5", "--termination-grace-period=0.5"],
        "child_ready",
    )?;

    assert_snapshot!(snapshot_output(&output), @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    error: run timed out before all tests completed

    ----- stderr -----
    "###);
    assert_descendants_stopped(&context);
    Ok(())
}

fn descendant_context(sleep_in_worker: bool) -> TestContext {
    let body = if sleep_in_worker {
        "time.sleep(30)"
    } else {
        "pass"
    };
    let context = TestContext::with_files([
        (
            "child.py",
            r#"
import os
import subprocess
import sys
import time
from pathlib import Path


grandchild = subprocess.Popen([sys.executable, "grandchild.py"])
Path("grandchild_pid").write_text(str(grandchild.pid))
while not Path("grandchild_ready").exists():
    time.sleep(0.01)
Path("child_ready").write_text("1")
time.sleep(30)
"#,
        ),
        (
            "grandchild.py",
            r#"
import time
from pathlib import Path

Path("grandchild_ready").write_text("1")
time.sleep(30)
"#,
        ),
    ]);
    context.write_file(
        "test.py",
        &format!(
            r#"
import os
import subprocess
import sys
import time
from pathlib import Path


def test_starts_child():
    worker_pid = os.getpid()
    child = subprocess.Popen([sys.executable, "child.py"])
    Path("child_pid").write_text(str(child.pid))
    Path("worker_pid").write_text(str(worker_pid))
    while not Path("child_ready").exists():
        time.sleep(0.01)
    {body}
"#
        ),
    );
    context
}

fn run_karva(context: &TestContext, args: &[&str]) -> Result<Output, String> {
    let mut command = context.command();
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let karva = command.spawn().expect("spawn Karva");
    collect_karva(karva, args)
}

fn run_karva_when_ready(
    context: &TestContext,
    args: &[&str],
    marker: &str,
) -> Result<Output, String> {
    let mut command = context.command();
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut karva = command.spawn().expect("spawn Karva");
    // Receipt: five seconds covers worker startup and the child readiness
    // handshake measured by the existing lifecycle integration tests.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !context.root().join(marker).exists() {
        if karva.try_wait().expect("poll Karva").is_some() {
            let output = karva.wait_with_output().expect("collect Karva output");
            return Err(format!(
                "Karva exited before {marker}:\n{}",
                snapshot_output(&output)
            ));
        }
        thread::sleep(Duration::from_millis(10));
    }
    if !context.root().join(marker).exists() {
        let _ = Command::new(if cfg!(windows) { "taskkill" } else { "kill" })
            .args(if cfg!(windows) {
                vec![
                    "/PID".to_string(),
                    karva.id().to_string(),
                    "/T".to_string(),
                    "/F".to_string(),
                ]
            } else {
                vec!["-KILL".to_string(), karva.id().to_string()]
            })
            .status();
        let output = karva
            .wait_with_output()
            .expect("collect stalled Karva output");
        return Err(format!(
            "child did not become ready before the timeout test:\n{}",
            snapshot_output(&output)
        ));
    }
    collect_karva(karva, args)
}

fn collect_karva(mut karva: std::process::Child, args: &[&str]) -> Result<Output, String> {
    // Receipt: ten seconds leaves a five-second cleanup budget after the timeout
    // and covers the five-second
    // descendant observation window with room for CI scheduling.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if karva.try_wait().expect("poll Karva").is_some() {
            return Ok(karva.wait_with_output().expect("collect Karva output"));
        }
        if Instant::now() >= deadline {
            let _ = Command::new(if cfg!(windows) { "taskkill" } else { "kill" })
                .args(if cfg!(windows) {
                    vec![
                        "/PID".to_string(),
                        karva.id().to_string(),
                        "/T".to_string(),
                        "/F".to_string(),
                    ]
                } else {
                    vec!["-KILL".to_string(), karva.id().to_string()]
                })
                .status();
            let output = karva
                .wait_with_output()
                .expect("collect stalled Karva output");
            return Err(format!(
                "Karva stalled: {args:?}\n{}",
                snapshot_output(&output)
            ));
        }
        thread::sleep(Duration::from_millis(10));
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

fn assert_descendants_stopped(context: &TestContext) {
    let child_pid = context.read_file("child_pid");
    let grandchild_pid = context.read_file("grandchild_pid");
    // Receipt: five seconds allows asynchronous process teardown to become
    // visible before reporting a leaked descendant.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline
        && (process_running(child_pid.trim()) || process_running(grandchild_pid.trim()))
    {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !process_running(child_pid.trim()),
        "child {child_pid} survived"
    );
    assert!(
        !process_running(grandchild_pid.trim()),
        "grandchild {grandchild_pid} survived"
    );
}

fn process_running(pid: &str) -> bool {
    if cfg!(windows) {
        let output = Command::new("tasklist")
            .args([
                "/FI",
                &format!("PID eq {pid}"),
                concat!("/F", "O"),
                "CSV",
                "/NH",
            ])
            .output()
            .expect("inspect Windows descendant");
        let row = format!("\",\"{pid}\",");
        String::from_utf8_lossy(&output.stdout).contains(&row)
    } else {
        Command::new("kill")
            .args(["-0", pid])
            .status()
            .is_ok_and(|status| status.success())
    }
}
