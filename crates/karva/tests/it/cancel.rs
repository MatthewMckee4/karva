#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use camino::Utf8Path;
use insta::assert_snapshot;

use crate::common::TestContext;

#[test]
fn test_ctrlc_emits_cancellation_banner() {
    let context = TestContext::with_file(
        "test_slow.py",
        r"
from pathlib import Path
import time

def test_slow():
    Path('started').write_text('1')
    time.sleep(60)
",
    );
    let started_file = context.root().join("started");

    let mut child = context
        .command()
        .args(["--num-workers", "1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn karva");

    let pid = child.id();

    if let Err(message) = wait_for_file(&started_file, Duration::from_secs(20)) {
        let _ = child.kill();
        let output = child
            .wait_with_output()
            .expect("Failed to wait on karva process");
        panic!(
            "{message}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let status = Command::new("kill")
        .args(["-s", "INT", &pid.to_string()])
        .status()
        .expect("Failed to invoke kill");
    assert!(status.success(), "kill -s INT {pid} failed");

    let output = child
        .wait_with_output()
        .expect("Failed to wait on karva process");

    let stdout = String::from_utf8_lossy(&output.stdout);

    assert_snapshot!(stdout, @r"
        Starting 1 test across 1 worker
      Cancelling due to interrupt: 1 test still running
          SIGINT [TIME] test_slow::test_slow
    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped
    ");
}

fn wait_for_file(path: &Utf8Path, timeout: Duration) -> Result<(), String> {
    let start = Instant::now();

    while start.elapsed() < timeout {
        if path.exists() {
            return Ok(());
        }

        std::thread::sleep(Duration::from_millis(25));
    }

    Err(format!("Timed out waiting for `{path}`"))
}
