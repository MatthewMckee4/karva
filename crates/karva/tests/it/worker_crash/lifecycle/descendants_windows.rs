#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::common::TestContext;
#[cfg(windows)]
use windows_sys::Win32::System::Console::{CTRL_BREAK_EVENT, GenerateConsoleCtrlEvent};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::CREATE_NEW_PROCESS_GROUP;

#[test]
fn completed_worker_kills_windows_grandchild() {
    let context = descendant_context(false);
    let output = run_karva(&context, &[]);
    assert_status(&output, true);
    assert_descendants_stopped(&context);
}

#[test]
fn timed_out_worker_kills_windows_grandchild_after_grace() {
    let context = descendant_context(true);
    // Receipt: ten seconds gives cold Windows workers time to publish both
    // readiness files before the half-second grace window is exercised.
    let output = run_karva_when_ready(
        &context,
        &["--run-timeout=10", "--termination-grace-period=0.5"],
        "child_ready",
    );
    assert_status(&output, false);
    assert_marker(&context, "child_after_worker");
    assert_descendants_stopped(&context);
}

#[test]
fn interrupted_worker_kills_windows_grandchild() {
    let context = descendant_context(true);
    let mut command = context.command();
    command
        .args(["--termination-grace-period=0"])
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut karva = command.spawn().expect("spawn Karva");
    wait_for_marker(&context, "child_ready", &mut karva);

    #[expect(
        unsafe_code,
        reason = "Windows console control is the interrupt test boundary"
    )]
    let sent = unsafe { GenerateConsoleCtrlEvent(CTRL_BREAK_EVENT, karva.id()) };
    assert_ne!(sent, 0, "send Ctrl+Break to Karva process group");
    let output = karva
        .wait_with_output()
        .expect("collect interrupted Karva output");

    assert_status(&output, false);
    assert_descendants_stopped(&context);
}

#[test]
fn fail_fast_worker_kills_windows_grandchild() {
    let context = fail_fast_context();
    let output = run_karva(
        &context,
        &[
            "--num-workers=2",
            "--max-fail=1",
            "--termination-grace-period=0",
        ],
    );

    assert_status(&output, false);
    assert_descendants_stopped(&context);
}

fn descendant_context(sleep_in_worker: bool) -> TestContext {
    // Receipt: thirty seconds exceeds the one-second timeout and half-second
    // grace used by the timeout test, keeping an uncontained process observable.
    let body = if sleep_in_worker {
        "time.sleep(30)"
    } else {
        "pass"
    };
    let context = TestContext::with_files([
        (
            "child.py",
            r#"
import subprocess
import sys
import time
from pathlib import Path


grandchild = subprocess.Popen([sys.executable, "grandchild.py"])
Path("grandchild_pid").write_text(str(grandchild.pid))
while not Path("grandchild_ready").exists():
    time.sleep(0.01)
Path("child_ready").write_text("1")
worker_pid = Path("worker_pid").read_text().strip()
while any(
    line.startswith('"') and f'","{worker_pid}","' in line
    for line in subprocess.run(
        ["tasklist", "/F" + "O", "CSV", "/NH"],
        capture_output=True,
        text=True,
        check=True,
    ).stdout.splitlines()
):
    time.sleep(0.01)
Path("child_after_worker").write_text("1")
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
import subprocess
import sys
import time
from pathlib import Path


def test_starts_child():
    Path("worker_pid").write_text(str(__import__("os").getpid()))
    child = subprocess.Popen([sys.executable, "child.py"])
    Path("child_pid").write_text(str(child.pid))
    while not Path("child_ready").exists():
        time.sleep(0.01)
    {body}
"#
        ),
    );
    context
}

fn fail_fast_context() -> TestContext {
    TestContext::with_files([
        (
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


# Receipt: ten tests keep both workers above Karva's five-test minimum.
def test_padding_0():
    pass


def test_padding_1():
    pass


def test_padding_2():
    pass


def test_padding_3():
    pass


def test_padding_4():
    pass


def test_padding_5():
    pass


def test_padding_6():
    pass


def test_padding_7():
    pass


def test_starts_child():
    Path("worker_pid").write_text(str(__import__("os").getpid()))
    child = subprocess.Popen([sys.executable, "child.py"])
    Path("child_pid").write_text(str(child.pid))
    while not Path("child_ready").exists():
        time.sleep(0.01)
    time.sleep(30)
"#,
        ),
        (
            "child.py",
            r#"
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
    ])
}

fn wait_for_marker(context: &TestContext, marker: &str, karva: &mut std::process::Child) {
    // Receipt: five seconds covers worker startup and the child readiness handshake in CI.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !context.root().join(marker).exists() {
        assert!(
            karva.try_wait().expect("poll Karva").is_none(),
            "Karva exited before {marker}"
        );
        thread::sleep(Duration::from_millis(10));
    }
    if context.root().join(marker).exists() {
        return;
    }
    let _ = Command::new("taskkill")
        .args(["/PID", &karva.id().to_string(), "/T", "/F"])
        .status();
    assert!(
        context.root().join(marker).exists(),
        "Windows child did not become ready: {marker}"
    );
}

fn run_karva_when_ready(context: &TestContext, args: &[&str], marker: &str) -> Output {
    let mut command = context.command();
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut karva = command.spawn().expect("spawn Karva");
    wait_for_marker(context, marker, &mut karva);
    collect_karva(karva, args)
}

fn run_karva(context: &TestContext, args: &[&str]) -> Output {
    let mut command = context.command();
    command
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let karva = command.spawn().expect("spawn Karva");
    collect_karva(karva, args)
}

fn collect_karva(mut karva: std::process::Child, args: &[&str]) -> Output {
    // Receipt: ten seconds covers the existing timeout budget and the
    // five-second descendant observation window with room for CI scheduling.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if karva.try_wait().expect("poll Karva").is_some() {
            return karva.wait_with_output().expect("collect Karva output");
        }
        if Instant::now() >= deadline {
            let _ = Command::new("taskkill")
                .args(["/PID", &karva.id().to_string(), "/T", "/F"])
                .status();
            let output = karva
                .wait_with_output()
                .expect("collect stalled Karva output");
            assert!(false, "Karva stalled: {args:?}\n{}", output_report(&output));
            return output;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

fn assert_status(output: &Output, expected_success: bool) {
    assert_eq!(
        output.status.success(),
        expected_success,
        "{}",
        output_report(output),
    );
}

fn output_report(output: &Output) -> String {
    format!(
        "status: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    )
}

fn assert_marker(context: &TestContext, marker: &str) {
    // Receipt: the half-second grace period is long enough for the child to
    // observe the worker exit and publish this marker before force-kill.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !context.root().join(marker).exists() {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        context.root().join(marker).exists(),
        "Windows descendant did not observe worker exit during grace: {marker}"
    );
}

fn assert_descendants_stopped(context: &TestContext) {
    let child_pid = context.read_file("child_pid");
    let grandchild_pid = context.read_file("grandchild_pid");
    // Receipt: five seconds allows TerminateJobObject's asynchronous process
    // teardown to become visible before reporting a leaked descendant.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline
        && (process_running(child_pid.trim()) || process_running(grandchild_pid.trim()))
    {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !process_running(child_pid.trim()),
        "Windows child {child_pid} survived"
    );
    assert!(
        !process_running(grandchild_pid.trim()),
        "Windows grandchild {grandchild_pid} survived"
    );
}

fn process_running(pid: &str) -> bool {
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
}
