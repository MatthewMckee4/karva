use std::io::Write;
use std::process::Stdio;

use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_snapshot_review_accept() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    let _ = context.command_no_parallel().output();

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("a\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/1
    File: <temp_dir>/snapshots/test__test_hello.snap.new
    Source: test.py:5::test_hello

    [LONG-LINE]┬[LONG-LINE]
              1 | +hello world
    [LONG-LINE]┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    > 
    insta review finished
    accepted:
      <temp_dir>/snapshots/test__test_hello.snap.new

    ----- stderr -----
    ");

    let snap_path = context.root().join("snapshots/test__test_hello.snap");
    let snap_new_path = context.root().join("snapshots/test__test_hello.snap.new");
    assert!(snap_path.exists(), "Expected .snap file after accept");
    assert!(
        !snap_new_path.exists(),
        "Expected .snap.new file to be removed after accept"
    );
}

#[test]
fn test_snapshot_review_reject() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    let _ = context.command_no_parallel().output();

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("r\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/1
    File: <temp_dir>/snapshots/test__test_hello.snap.new
    Source: test.py:5::test_hello

    [LONG-LINE]┬[LONG-LINE]
              1 | +hello world
    [LONG-LINE]┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    > 
    insta review finished
    rejected:
      <temp_dir>/snapshots/test__test_hello.snap.new

    ----- stderr -----
    ");

    let snap_path = context.root().join("snapshots/test__test_hello.snap");
    let snap_new_path = context.root().join("snapshots/test__test_hello.snap.new");
    assert!(!snap_path.exists(), "Expected no .snap file after reject");
    assert!(
        !snap_new_path.exists(),
        "Expected .snap.new file to be removed after reject"
    );
}

#[test]
fn test_snapshot_review_skip() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    let _ = context.command_no_parallel().output();

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("s\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/1
    File: <temp_dir>/snapshots/test__test_hello.snap.new
    Source: test.py:5::test_hello

    [LONG-LINE]┬[LONG-LINE]
              1 | +hello world
    [LONG-LINE]┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    > 
    insta review finished
    skipped:
      <temp_dir>/snapshots/test__test_hello.snap.new

    ----- stderr -----
    ");

    let snap_new_path = context.root().join("snapshots/test__test_hello.snap.new");
    assert!(
        snap_new_path.exists(),
        "Expected .snap.new file to still exist after skip"
    );
}

#[test]
fn test_snapshot_review_skip_all() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_one():
    karva.assert_snapshot('first')

def test_two():
    karva.assert_snapshot('second')
        ",
    );

    let _ = context.command_no_parallel().output();

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("S\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/2
    File: <temp_dir>/snapshots/test__test_one.snap.new
    Source: test.py:5::test_one

    [LONG-LINE]┬[LONG-LINE]
              1 | +first
    [LONG-LINE]┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    > 
    insta review finished
    skipped:
      <temp_dir>/snapshots/test__test_one.snap.new
      <temp_dir>/snapshots/test__test_two.snap.new

    ----- stderr -----
    ");

    let snap_new_one = context.root().join("snapshots/test__test_one.snap.new");
    let snap_new_two = context.root().join("snapshots/test__test_two.snap.new");
    assert!(
        snap_new_one.exists(),
        "Expected .snap.new file to still exist after skip all"
    );
    assert!(
        snap_new_two.exists(),
        "Expected .snap.new file to still exist after skip all"
    );
}

#[test]
fn test_snapshot_review_no_pending() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    assert_cmd_snapshot!(context.snapshot("review"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    No pending snapshots to review.

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_review_accept_all() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_alpha():
    karva.assert_snapshot('alpha')

def test_beta():
    karva.assert_snapshot('beta')
        ",
    );

    let _ = context.command_no_parallel().output();

    let mut child = context
        .snapshot("review")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn");
    child
        .stdin
        .take()
        .expect("no stdin")
        .write_all(b"A\n")
        .expect("write failed");
    let _ = child.wait_with_output();

    let snap_alpha = context.root().join("snapshots/test__test_alpha.snap");
    let snap_beta = context.root().join("snapshots/test__test_beta.snap");
    let pending_alpha = context.root().join("snapshots/test__test_alpha.snap.new");
    let pending_beta = context.root().join("snapshots/test__test_beta.snap.new");

    assert!(snap_alpha.exists(), "Expected alpha .snap after accept all");
    assert!(snap_beta.exists(), "Expected beta .snap after accept all");
    assert!(
        !pending_alpha.exists(),
        "Expected alpha .snap.new removed after accept all"
    );
    assert!(
        !pending_beta.exists(),
        "Expected beta .snap.new removed after accept all"
    );
}

#[test]
fn test_snapshot_review_reject_all() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_alpha():
    karva.assert_snapshot('alpha')

def test_beta():
    karva.assert_snapshot('beta')
        ",
    );

    let _ = context.command_no_parallel().output();

    let mut child = context
        .snapshot("review")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn");
    child
        .stdin
        .take()
        .expect("no stdin")
        .write_all(b"R\n")
        .expect("write failed");
    let _ = child.wait_with_output();

    let snap_alpha = context.root().join("snapshots/test__test_alpha.snap");
    let snap_beta = context.root().join("snapshots/test__test_beta.snap");
    let pending_alpha = context.root().join("snapshots/test__test_alpha.snap.new");
    let pending_beta = context.root().join("snapshots/test__test_beta.snap.new");

    assert!(
        !snap_alpha.exists(),
        "Expected no alpha .snap after reject all"
    );
    assert!(
        !snap_beta.exists(),
        "Expected no beta .snap after reject all"
    );
    assert!(
        !pending_alpha.exists(),
        "Expected alpha .snap.new removed after reject all"
    );
    assert!(
        !pending_beta.exists(),
        "Expected beta .snap.new removed after reject all"
    );
}

#[test]
fn test_snapshot_review_mixed_actions() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_alpha():
    karva.assert_snapshot('alpha')

def test_beta():
    karva.assert_snapshot('beta')
        ",
    );

    let _ = context.command_no_parallel().output();

    let mut child = context
        .snapshot("review")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn");
    child
        .stdin
        .take()
        .expect("no stdin")
        .write_all(b"a\nr\n")
        .expect("write failed");
    let _ = child.wait_with_output();

    let snap_alpha = context.root().join("snapshots/test__test_alpha.snap");
    let snap_beta = context.root().join("snapshots/test__test_beta.snap");
    let pending_alpha = context.root().join("snapshots/test__test_alpha.snap.new");
    let pending_beta = context.root().join("snapshots/test__test_beta.snap.new");

    assert!(snap_alpha.exists(), "Expected alpha .snap after accept");
    assert!(!snap_beta.exists(), "Expected no beta .snap after reject");
    assert!(!pending_alpha.exists(), "Expected alpha .snap.new removed");
    assert!(!pending_beta.exists(), "Expected beta .snap.new removed");
}
