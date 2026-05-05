#![cfg(unix)]

use std::process::{Command, Stdio};
use std::time::Duration;

use insta::assert_snapshot;

use crate::common::TestContext;

#[test]
fn test_ctrlc_emits_cancellation_banner() {
    // Mix of fast tests (which complete and print PASS lines) and slow
    // tests (which keep workers busy when SIGINT arrives) so the snapshot
    // exercises both code paths and shows non-trivial output.
    let context = TestContext::with_file(
        "test_mixed.py",
        r"
import time

def test_fast_a(): pass
def test_fast_b(): pass
def test_fast_c(): pass
def test_fast_d(): pass
def test_fast_e(): pass
def test_slow_a(): time.sleep(60)
def test_slow_b(): time.sleep(60)
def test_slow_c(): time.sleep(60)
def test_slow_d(): time.sleep(60)
def test_slow_e(): time.sleep(60)
",
    );

    let child = context
        .command()
        .args(["--num-workers", "2"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to spawn karva");

    let pid = child.id();

    // Wait long enough for karva to launch its workers, run the fast
    // tests, and reach the wait-for-completion loop blocked on the slow
    // tests. The slow tests sleep for 60s so karva will still be running
    // when we send the signal.
    std::thread::sleep(Duration::from_secs(5));

    let status = Command::new("kill")
        .args(["-s", "INT", &pid.to_string()])
        .status()
        .expect("Failed to invoke kill");
    assert!(status.success(), "kill -s INT {pid} failed");

    let output = child
        .wait_with_output()
        .expect("Failed to wait on karva process");

    let mut stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    // Worker scheduling means PASS and SIGINT lines can appear in any
    // order. Sort each block independently for a deterministic snapshot.
    // The ordering of every other line (Starting / Cancelling / summary
    // / error) is deterministic.
    sort_block_starting_with(&mut stdout, "PASS");
    sort_block_starting_with(&mut stdout, "SIGINT");

    assert_snapshot!(stdout, @r"
        Starting 10 tests across 2 workers
            PASS [TIME] test_mixed::test_fast_a
            PASS [TIME] test_mixed::test_fast_b
            PASS [TIME] test_mixed::test_fast_c
            PASS [TIME] test_mixed::test_fast_d
            PASS [TIME] test_mixed::test_fast_e
      Cancelling due to interrupt: 10 tests still running
          SIGINT [TIME] test_mixed::test_slow_a
          SIGINT [TIME] test_mixed::test_slow_b
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped
    error: no tests to run
    (hint: use `--no-tests` to customize)
    ");
}

/// Sort the contiguous block of lines whose first token is `label` so
/// the snapshot is deterministic. Workers run in parallel so PASS- and
/// SIGINT-line ordering is racy, but every other line is emitted by
/// the orchestrator in a fixed order.
fn sort_block_starting_with(stdout: &mut String, label: &str) {
    let lines: Vec<&str> = stdout.lines().collect();
    let first = lines.iter().position(|l| l.trim_start().starts_with(label));
    let Some(start) = first else { return };
    let end = start
        + lines[start..]
            .iter()
            .take_while(|l| l.trim_start().starts_with(label))
            .count();
    let mut sorted: Vec<String> = lines[start..end].iter().map(ToString::to_string).collect();
    sorted.sort();
    let mut rebuilt = lines[..start].join("\n");
    if !rebuilt.is_empty() {
        rebuilt.push('\n');
    }
    rebuilt.push_str(&sorted.join("\n"));
    rebuilt.push('\n');
    rebuilt.push_str(&lines[end..].join("\n"));
    if stdout.ends_with('\n') {
        rebuilt.push('\n');
    }
    *stdout = rebuilt;
}
