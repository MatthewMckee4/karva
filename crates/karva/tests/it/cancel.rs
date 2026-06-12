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
    // Which two of the five slow tests are in flight when SIGINT arrives
    // depends on partitioning and timing, so collapse the suffix to keep
    // the snapshot stable across runs.
    stdout = regex::Regex::new(r"test_slow_[a-e]")
        .unwrap()
        .replace_all(&stdout, "test_slow_X")
        .into_owned();
    // Worker scheduling means PASS and SIGINT lines can appear in any
    // order. Sort each status independently for a deterministic snapshot.
    // The ordering of every other line (Starting / Cancelling / summary)
    // is deterministic.
    sort_lines_starting_with(&mut stdout, "PASS");
    sort_lines_starting_with(&mut stdout, "SIGINT");

    assert_snapshot!(stdout, @r"
        Starting 10 tests across 2 workers
            PASS [TIME] test_mixed::test_fast_a
            PASS [TIME] test_mixed::test_fast_b
            PASS [TIME] test_mixed::test_fast_c
            PASS [TIME] test_mixed::test_fast_d
            PASS [TIME] test_mixed::test_fast_e
      Cancelling due to interrupt: 2 tests still running
          SIGINT [TIME] test_mixed::test_slow_X
          SIGINT [TIME] test_mixed::test_slow_X
    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped
    ");
}

/// Sort all lines whose first token is `label` so the snapshot is deterministic.
fn sort_lines_starting_with(stdout: &mut String, label: &str) {
    let mut lines: Vec<String> = stdout.lines().map(ToString::to_string).collect();
    let positions: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| line.trim_start().starts_with(label).then_some(index))
        .collect();
    let mut sorted: Vec<String> = positions
        .iter()
        .map(|index| lines[*index].clone())
        .collect();
    sorted.sort();

    for (position, line) in positions.into_iter().zip(sorted) {
        lines[position] = line;
    }

    let mut rebuilt = lines.join("\n");
    if stdout.ends_with('\n') {
        rebuilt.push('\n');
    }
    *stdout = rebuilt;
}
