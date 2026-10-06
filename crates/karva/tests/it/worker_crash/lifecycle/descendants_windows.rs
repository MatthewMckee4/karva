use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::common::TestContext;

#[test]
fn completed_worker_kills_windows_descendant() {
    let context = descendant_context(false);
    let (output, _) = run_karva(&context, &[]);

    assert_status(&output, true);
    assert_descendant_stopped(&context);
}

#[test]
fn timed_out_worker_kills_windows_descendant() {
    let context = descendant_context(true);
    let (output, elapsed) = run_karva(
        &context,
        &["--run-timeout=1", "--termination-grace-period=0.5"],
    );

    assert_status(&output, false);
    assert!(
        elapsed >= Duration::from_millis(400),
        "Windows termination grace period was skipped: {elapsed:?}"
    );
    assert_descendant_stopped(&context);
}

#[test]
fn fail_fast_worker_kills_windows_descendant() {
    let context = fail_fast_context();
    let (output, _) = run_karva(
        &context,
        &[
            "--num-workers=2",
            "--max-fail=1",
            "--termination-grace-period=0",
        ],
    );

    assert_status(&output, false);
    assert_descendant_stopped(&context);
}

fn descendant_context(sleep_in_worker: bool) -> TestContext {
    let body = if sleep_in_worker {
        "time.sleep(30)"
    } else {
        "pass"
    };
    TestContext::with_file(
        "test.py",
        &format!(
            r#"
import subprocess
import sys
import time
from pathlib import Path


def test_starts_child():
    child = subprocess.Popen([
        sys.executable,
        "-c",
        "from pathlib import Path; import time; Path('child_ready').write_text('1'); time.sleep(30)",
    ])
    Path("child_pid").write_text(str(child.pid))
    while not Path("child_ready").exists():
        time.sleep(0.01)
    {body}
"#
        ),
    )
}

fn fail_fast_context() -> TestContext {
    TestContext::with_file(
        "test.py",
        r#"
import subprocess
import sys
import time
from pathlib import Path


def test_fails():
    while not Path("child_ready").exists():
        time.sleep(0.01)
    assert False


def test_starts_child():
    child = subprocess.Popen([
        sys.executable,
        "-c",
        "from pathlib import Path; import time; Path('child_ready').write_text('1'); time.sleep(30)",
    ])
    Path("child_pid").write_text(str(child.pid))
    while not Path("child_ready").exists():
        time.sleep(0.01)
    time.sleep(30)
"#,
    )
}

fn run_karva(context: &TestContext, args: &[&str]) -> (Output, Duration) {
    let mut command = context.command();
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut karva = command.spawn().expect("spawn Karva");
    let started = Instant::now();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if karva.try_wait().expect("poll Karva").is_some() {
            return (
                karva.wait_with_output().expect("collect Karva output"),
                started.elapsed(),
            );
        }
        if Instant::now() >= deadline {
            let _ = Command::new("taskkill")
                .args(["/PID", &karva.id().to_string(), "/T", "/F"])
                .status();
            let output = karva
                .wait_with_output()
                .expect("collect stalled Karva output");
            panic!(
                "Karva stalled: {args:?}\nstatus: {}\nstdout:\n{}\nstderr:\n{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn assert_status(output: &Output, expected_success: bool) {
    assert_eq!(
        output.status.success(),
        expected_success,
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn assert_descendant_stopped(context: &TestContext) {
    let pid = context.read_file("child_pid");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && process_running(pid.trim()) {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !process_running(pid.trim()),
        "Windows descendant {pid} survived"
    );
}

fn process_running(pid: &str) -> bool {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .expect("inspect Windows descendant");
    let row = format!("\",\"{pid}\",");
    String::from_utf8_lossy(&output.stdout).contains(&row)
}
