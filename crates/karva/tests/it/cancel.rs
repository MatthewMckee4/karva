#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::Duration;

use crate::common::TestContext;

#[test]
fn test_ctrlc_emits_cancellation_banner() {
    let context = TestContext::with_file(
        "test_slow.py",
        r"
import time

def test_slow():
    time.sleep(60)
",
    );

    let child = context
        .command()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn karva");

    let pid = child.id();

    // Wait long enough for karva to launch its workers and reach the
    // wait-for-completion loop. The slow test sleeps for 60s so karva will
    // still be running when we send the signal.
    std::thread::sleep(Duration::from_secs(5));

    let status = Command::new("kill")
        .args(["-s", "INT", &pid.to_string()])
        .status()
        .expect("Failed to invoke kill");
    assert!(status.success(), "kill -s INT {pid} failed");

    let output = child
        .wait_with_output()
        .expect("Failed to wait on karva process");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("Cancelling due to interrupt"),
        "expected cancellation banner in stdout, got:\n{stdout}"
    );
    assert!(
        stdout.contains("SIGINT"),
        "expected SIGINT line in stdout, got:\n{stdout}"
    );
}
