use std::io::Write;
use std::process::Stdio;

use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_snapshot_creates_snap_new_file() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    // First run: no existing snapshot, creates .snap.new and fails
    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_hello ... FAILED

    diagnostics:

    error[test-failure]: Test `test_hello` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_hello():
      |     ^^^^^^^^^^
    5 |     karva.assert_snapshot('hello world')
      |
    info: Test failed here
     --> test.py:5:5
      |
    4 | def test_hello():
    5 |     karva.assert_snapshot('hello world')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: New snapshot for 'test_hello'.
          Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.
          Pending file: <temp_dir>/snapshots/test__test_hello.snap.new

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_hello.snap.new");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_hello
    ---
    hello world
    ");
}

#[test]
fn test_snapshot_update_creates_snap_file() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");

    assert_cmd_snapshot!(cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_hello ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_hello.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_hello
    ---
    hello world
    ");
}

#[test]
fn test_snapshot_matches() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    // First create the snapshot
    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    // Second run should match
    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_hello ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_mismatch() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    // Create the initial snapshot
    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    // Now change the test to produce different output
    context.write_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('goodbye world')
        ",
    );

    // Run again — should fail with mismatch
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_hello ... FAILED

    diagnostics:

    error[test-failure]: Test `test_hello` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_hello():
      |     ^^^^^^^^^^
    5 |     karva.assert_snapshot('goodbye world')
      |
    info: Test failed here
     --> test.py:5:5
      |
    4 | def test_hello():
    5 |     karva.assert_snapshot('goodbye world')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Snapshot mismatch for 'test_hello'.
          Snapshot file: <temp_dir>/snapshots/test__test_hello.snap
          [LONG-LINE]┬[LONG-LINE]
              1       | -hello world
                    1 | +goodbye world
          [LONG-LINE]┴[LONG-LINE]

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_multiple_per_test() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_multi():
    karva.assert_snapshot('first')
    karva.assert_snapshot('second')
    karva.assert_snapshot('third')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");

    assert_cmd_snapshot!(cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_multi ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content_1 = context.read_file("snapshots/test__test_multi.snap");
    insta::assert_snapshot!(content_1, @r"
    ---
    source: test.py:5::test_multi
    ---
    first
    ");

    let content_2 = context.read_file("snapshots/test__test_multi-2.snap");
    insta::assert_snapshot!(content_2, @r"
    ---
    source: test.py:6::test_multi
    ---
    second
    ");

    let content_3 = context.read_file("snapshots/test__test_multi-3.snap");
    insta::assert_snapshot!(content_3, @r"
    ---
    source: test.py:7::test_multi
    ---
    third
    ");
}

#[test]
fn test_snapshot_accept_command() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    // Run tests to create .snap.new
    let _ = context.command_no_parallel().output();

    // Run accept command
    assert_cmd_snapshot!(context.snapshot("accept"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Accepted: <temp_dir>/snapshots/test__test_hello.snap.new

    1 snapshot(s) accepted.

    ----- stderr -----
    ");

    // Verify .snap file content and .snap.new is gone
    let content = context.read_file("snapshots/test__test_hello.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_hello
    ---
    hello world
    ");

    let snap_new_path = context.root().join("snapshots/test__test_hello.snap.new");
    assert!(
        !snap_new_path.exists(),
        "Expected .snap.new file to be removed after accept"
    );
}

#[test]
fn test_snapshot_reject_command() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    // Run tests to create .snap.new
    let _ = context.command_no_parallel().output();

    // Run reject command
    assert_cmd_snapshot!(context.snapshot("reject"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Rejected: <temp_dir>/snapshots/test__test_hello.snap.new

    1 snapshot(s) rejected.

    ----- stderr -----
    ");

    // Verify .snap.new is gone and no .snap was created
    let snap_path = context.root().join("snapshots/test__test_hello.snap");
    let snap_new_path = context.root().join("snapshots/test__test_hello.snap.new");
    assert!(!snap_path.exists(), "Expected no .snap file after reject");
    assert!(
        !snap_new_path.exists(),
        "Expected .snap.new file to be removed after reject"
    );
}

#[test]
fn test_snapshot_pending_command() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    // Run tests to create .snap.new
    let _ = context.command_no_parallel().output();

    // Run pending command
    assert_cmd_snapshot!(context.snapshot("pending"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    <temp_dir>/snapshots/test__test_hello.snap.new

    1 pending snapshot(s).

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_parametrized() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.parametrize('x', [1, 2])
def test_param(x):
    karva.assert_snapshot(str(x))
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");

    assert_cmd_snapshot!(cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_param(x=1) ... ok
    test test::test_param(x=2) ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_param(x=1).snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:6::test_param(x=1)
    ---
    1
    ");
}

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

    // Run tests to create .snap.new
    let _ = context.command_no_parallel().output();

    // Pipe 'a' to review to accept
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

    // Verify .snap exists and .snap.new is gone
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

    // Run tests to create .snap.new
    let _ = context.command_no_parallel().output();

    // Pipe 'r' to review to reject
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

    // Verify both are gone
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

    // Run tests to create .snap.new
    let _ = context.command_no_parallel().output();

    // Pipe 's' to review to skip
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

    // Verify .snap.new still exists
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

    // Run tests to create .snap.new files
    let _ = context.command_no_parallel().output();

    // Pipe 'S' to review to skip all
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

    // Verify both .snap.new files still exist
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

    // Don't run tests, so no .snap.new files exist
    assert_cmd_snapshot!(context.snapshot("review"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    No pending snapshots to review.

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_multiline_content() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_lines():
    karva.assert_snapshot('line one\nline two\nline three')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    let content = context.read_file("snapshots/test__test_lines.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_lines
    ---
    line one
    line two
    line three
    ");
}

#[test]
fn test_snapshot_content_with_leading_trailing_spaces() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_spaces():
    karva.assert_snapshot('  hello  ')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    let content = context.read_file("snapshots/test__test_spaces.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_spaces
    ---
      hello
    ");
}

#[test]
fn test_snapshot_content_with_tabs_and_mixed_whitespace() {
    let context = TestContext::with_file(
        "test.py",
        "
import karva

def test_tabs():
    karva.assert_snapshot('col1\\tcol2\\tcol3\\n  indented\\n\\ttab indented')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    let content = context.read_file("snapshots/test__test_tabs.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_tabs
    ---
    col1	col2	col3
      indented
    	tab indented
    ");
}

#[test]
fn test_snapshot_empty_string() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_empty():
    karva.assert_snapshot('')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    let content = context.read_file("snapshots/test__test_empty.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_empty
    ---

    ");
}

#[test]
fn test_snapshot_unicode_content() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_unicode():
    karva.assert_snapshot('Hello \u00e9\u00e8\u00ea \u2603 \u2764 \u00fc\u00f1\u00ee\u00e7\u00f6d\u00e9')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    let content = context.read_file("snapshots/test__test_unicode.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_unicode
    ---
    Hello éèê ☃ ❤ üñîçödé
    ");
}

#[test]
fn test_snapshot_update_overwrites_existing() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_overwrite():
    karva.assert_snapshot('original')
        ",
    );

    // Create initial snapshot
    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    let content = context.read_file("snapshots/test__test_overwrite.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_overwrite
    ---
    original
    ");

    // Change the test content
    context.write_file(
        "test.py",
        r"
import karva

def test_overwrite():
    karva.assert_snapshot('updated')
        ",
    );

    // Run with --snapshot-update to overwrite
    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");

    assert_cmd_snapshot!(cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_overwrite ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_overwrite.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_overwrite
    ---
    updated
    ");
}

#[test]
fn test_snapshot_multiple_tests_mixed_results() {
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

    // Create snapshots for both tests
    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    // Change only one test
    context.write_file(
        "test.py",
        r"
import karva

def test_one():
    karva.assert_snapshot('first')

def test_two():
    karva.assert_snapshot('changed')
        ",
    );

    // Run without update — test_one passes, test_two fails
    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_one ... ok
    test test::test_two ... FAILED

    diagnostics:

    error[test-failure]: Test `test_two` failed
     --> test.py:7:5
      |
    5 |     karva.assert_snapshot('first')
    6 |
    7 | def test_two():
      |     ^^^^^^^^
    8 |     karva.assert_snapshot('changed')
      |
    info: Test failed here
     --> test.py:8:5
      |
    7 | def test_two():
    8 |     karva.assert_snapshot('changed')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Snapshot mismatch for 'test_two'.
          Snapshot file: <temp_dir>/snapshots/test__test_two.snap
          [LONG-LINE]┬[LONG-LINE]
              1       | -second
                    1 | +changed
          [LONG-LINE]┴[LONG-LINE]

    test result: FAILED. 1 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_multiline_mismatch() {
    let context = TestContext::with_file(
        "test.py",
        "
import karva

def test_poem():
    karva.assert_snapshot('roses are red\\nviolets are blue')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    context.write_file(
        "test.py",
        "
import karva

def test_poem():
    karva.assert_snapshot('roses are red\\nviolets are purple\\nsugar is sweet')
        ",
    );

    // Mismatch with multiline content shows diff
    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_poem ... FAILED

    diagnostics:

    error[test-failure]: Test `test_poem` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_poem():
      |     ^^^^^^^^^
    5 |     karva.assert_snapshot('roses are red/nviolets are purple/nsugar is sweet')
      |
    info: Test failed here
     --> test.py:5:5
      |
    4 | def test_poem():
    5 |     karva.assert_snapshot('roses are red/nviolets are purple/nsugar is sweet')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Snapshot mismatch for 'test_poem'.
          Snapshot file: <temp_dir>/snapshots/test__test_poem.snap
          [LONG-LINE]┬[LONG-LINE]
              1     1 |  roses are red
              2       | -violets are blue
                    2 | +violets are purple
                    3 | +sugar is sweet
          [LONG-LINE]┴[LONG-LINE]

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

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

    // 'A' accepts all remaining
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

    // Both .snap files should exist, no .snap.new remains
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

    // 'R' rejects all remaining
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

    // No .snap files, no .snap.new files
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

    // Accept first, reject second
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

#[test]
fn test_snapshot_accept_multiple_pending() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_first():
    karva.assert_snapshot('aaa')

def test_second():
    karva.assert_snapshot('bbb')

def test_third():
    karva.assert_snapshot('ccc')
        ",
    );

    let _ = context.command_no_parallel().output();

    assert_cmd_snapshot!(context.snapshot("accept"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Accepted: <temp_dir>/snapshots/test__test_first.snap.new
    Accepted: <temp_dir>/snapshots/test__test_second.snap.new
    Accepted: <temp_dir>/snapshots/test__test_third.snap.new

    3 snapshot(s) accepted.

    ----- stderr -----
    ");

    // All tests pass now
    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_first ... ok
    test test::test_second ... ok
    test test::test_third ... ok

    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_reject_multiple_pending() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_first():
    karva.assert_snapshot('aaa')

def test_second():
    karva.assert_snapshot('bbb')
        ",
    );

    let _ = context.command_no_parallel().output();

    assert_cmd_snapshot!(context.snapshot("reject"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Rejected: <temp_dir>/snapshots/test__test_first.snap.new
    Rejected: <temp_dir>/snapshots/test__test_second.snap.new

    2 snapshot(s) rejected.

    ----- stderr -----
    ");

    // No .snap files remain
    assert!(
        !context
            .root()
            .join("snapshots/test__test_first.snap")
            .exists(),
        "Expected no .snap after reject"
    );
    assert!(
        !context
            .root()
            .join("snapshots/test__test_second.snap")
            .exists(),
        "Expected no .snap after reject"
    );
}

#[test]
fn test_snapshot_pending_multiple() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_aaa():
    karva.assert_snapshot('aaa')

def test_bbb():
    karva.assert_snapshot('bbb')

def test_ccc():
    karva.assert_snapshot('ccc')
        ",
    );

    let _ = context.command_no_parallel().output();

    assert_cmd_snapshot!(context.snapshot("pending"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    <temp_dir>/snapshots/test__test_aaa.snap.new
    <temp_dir>/snapshots/test__test_bbb.snap.new
    <temp_dir>/snapshots/test__test_ccc.snap.new

    3 pending snapshot(s).

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_content_with_special_characters() {
    let context = TestContext::with_file(
        "test.py",
        "
import karva

def test_special():
    karva.assert_snapshot('angle <brackets> & ampersand\\n\"double quotes\"\\n$dollar @at #hash')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    let content = context.read_file("snapshots/test__test_special.snap");
    insta::assert_snapshot!(content, @r#"
    ---
    source: test.py:5::test_special
    ---
    angle <brackets> & ampersand
    "double quotes"
    $dollar @at #hash
    "#);
}

#[test]
fn test_snapshot_content_with_dashes_like_yaml() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_dashes():
    karva.assert_snapshot('---\nthis looks like yaml\n---')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    let content = context.read_file("snapshots/test__test_dashes.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_dashes
    ---
    ---
    this looks like yaml
    ---
    ");
}

#[test]
fn test_snapshot_multiple_files() {
    let context = TestContext::default();
    context.write_file(
        "test_one.py",
        r"
import karva

def test_from_one():
    karva.assert_snapshot('from file one')
        ",
    );
    context.write_file(
        "test_two.py",
        r"
import karva

def test_from_two():
    karva.assert_snapshot('from file two')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    let one = context.read_file("snapshots/test_one__test_from_one.snap");
    insta::assert_snapshot!(one, @r"
    ---
    source: test_one.py:5::test_from_one
    ---
    from file one
    ");

    let two = context.read_file("snapshots/test_two__test_from_two.snap");
    insta::assert_snapshot!(two, @r"
    ---
    source: test_two.py:5::test_from_two
    ---
    from file two
    ");
}

#[test]
fn test_inline_snapshot_creates_value() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_hello():
    karva.assert_snapshot("hello world", inline="")
        "#,
    );

    // With --snapshot-update, should rewrite source and pass
    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");

    assert_cmd_snapshot!(cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_hello ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    // Verify the source file was rewritten
    let source = context.read_file("test.py");
    insta::assert_snapshot!(source, @r#"
    import karva

    def test_hello():
        karva.assert_snapshot("hello world", inline="hello world")
    "#);
}

#[test]
fn test_inline_snapshot_matches() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_hello():
    karva.assert_snapshot("hello world", inline="hello world")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_hello ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_inline_snapshot_mismatch_no_update() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_hello():
    karva.assert_snapshot("goodbye", inline="hello")
        "#,
    );

    // Without --snapshot-update, should fail with diff
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_hello ... FAILED

    diagnostics:

    error[test-failure]: Test `test_hello` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_hello():
      |     ^^^^^^^^^^
    5 |     karva.assert_snapshot("goodbye", inline="hello")
      |
    info: Test failed here
     --> test.py:5:5
      |
    4 | def test_hello():
    5 |     karva.assert_snapshot("goodbye", inline="hello")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Inline snapshot mismatch for 'test_hello'.
          [LONG-LINE]┬[LONG-LINE]
              1       | -hello
                    1 | +goodbye
          [LONG-LINE]┴[LONG-LINE]

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);
}

#[test]
fn test_inline_snapshot_mismatch_updates_source() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_hello():
    karva.assert_snapshot("goodbye", inline="hello")
        "#,
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");

    assert_cmd_snapshot!(cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_hello ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    insta::assert_snapshot!(source, @r#"
    import karva

    def test_hello():
        karva.assert_snapshot("goodbye", inline="goodbye")
    "#);
}

#[test]
fn test_inline_snapshot_multiline() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_lines():
    karva.assert_snapshot("line 1\nline 2\nline 3", inline="")
        "#,
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");

    assert_cmd_snapshot!(cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_lines ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    insta::assert_snapshot!(source, @r#"

    import karva

    def test_lines():
        karva.assert_snapshot("line 1/nline 2/nline 3", inline="""/
            line 1
            line 2
            line 3
        """)
    "#);
}

#[test]
fn test_inline_snapshot_multiline_matches() {
    let context = TestContext::with_file(
        "test.py",
        "
import karva

def test_lines():
    karva.assert_snapshot(\"line 1\\nline 2\", inline=\"\"\"\\\n        line 1\n        line 2\n    \"\"\")\n",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_lines ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_inline_snapshot_multiple_per_test() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_multi():
    karva.assert_snapshot("first", inline="")
    karva.assert_snapshot("second", inline="")
        "#,
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");

    assert_cmd_snapshot!(cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_multi ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    insta::assert_snapshot!(source, @r#"
    import karva

    def test_multi():
        karva.assert_snapshot("first", inline="first")
        karva.assert_snapshot("second", inline="second")
    "#);
}

#[test]
fn test_inline_snapshot_accept() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_hello():
    karva.assert_snapshot("hello world", inline="")
        "#,
    );

    // Run without update to create .snap.new
    let _ = context.command_no_parallel().output();

    // Source should be unchanged
    let source_before = context.read_file("test.py");
    assert!(
        source_before.contains(r#"inline="""#),
        "Expected source to still have empty inline"
    );

    // Run accept command
    assert_cmd_snapshot!(context.snapshot("accept"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Accepted: <temp_dir>/snapshots/test__test_hello_inline_5.snap.new

    1 snapshot(s) accepted.

    ----- stderr -----
    ");

    // Verify source was rewritten
    let source_after = context.read_file("test.py");
    insta::assert_snapshot!(source_after, @r#"
    import karva

    def test_hello():
        karva.assert_snapshot("hello world", inline="hello world")
    "#);
}

#[test]
fn test_inline_snapshot_reject() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_hello():
    karva.assert_snapshot("hello world", inline="")
        "#,
    );

    // Run without update to create .snap.new
    let _ = context.command_no_parallel().output();

    // Run reject command
    assert_cmd_snapshot!(context.snapshot("reject"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Rejected: <temp_dir>/snapshots/test__test_hello_inline_5.snap.new

    1 snapshot(s) rejected.

    ----- stderr -----
    ");

    // Source should be unchanged
    let source = context.read_file("test.py");
    insta::assert_snapshot!(source, @r#"
    import karva

    def test_hello():
        karva.assert_snapshot("hello world", inline="")
    "#);
}

#[test]
fn test_inline_snapshot_pending() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_hello():
    karva.assert_snapshot("hello world", inline="")
        "#,
    );

    // Run without update to create .snap.new
    let _ = context.command_no_parallel().output();

    // Run pending command
    assert_cmd_snapshot!(context.snapshot("pending"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    <temp_dir>/snapshots/test__test_hello_inline_5.snap.new

    1 pending snapshot(s).

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_prune_removes_unreferenced() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    // Remove the test function
    context.write_file(
        "test.py",
        r"
import karva

def test_other():
    pass
        ",
    );

    assert_cmd_snapshot!(context.snapshot("prune"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Removed: <temp_dir>/snapshots/test__test_hello.snap (function `test_hello` not found in test.py)

    1 snapshot(s) pruned.

    ----- stderr -----
    warning: Prune uses static analysis and may not detect all unreferenced snapshots.
    ");

    assert!(
        !context
            .root()
            .join("snapshots/test__test_hello.snap")
            .exists(),
        "Expected snapshot to be removed"
    );
}

#[test]
fn test_snapshot_prune_dry_run() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    // Remove the test function
    context.write_file(
        "test.py",
        r"
import karva

def test_other():
    pass
        ",
    );

    let mut prune_cmd = context.snapshot("prune");
    prune_cmd.arg("--dry-run");
    assert_cmd_snapshot!(prune_cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Would remove: <temp_dir>/snapshots/test__test_hello.snap (function `test_hello` not found in test.py)

    1 unreferenced snapshot(s) would be removed.

    ----- stderr -----
    warning: Prune uses static analysis and may not detect all unreferenced snapshots.
    ");

    assert!(
        context
            .root()
            .join("snapshots/test__test_hello.snap")
            .exists(),
        "Expected snapshot to still exist after dry run"
    );
}

#[test]
fn test_snapshot_prune_nothing_to_prune() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    assert_cmd_snapshot!(context.snapshot("prune"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    No unreferenced snapshots found.

    ----- stderr -----
    warning: Prune uses static analysis and may not detect all unreferenced snapshots.
    ");
}

#[test]
fn test_snapshot_prune_test_file_deleted() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    std::fs::remove_file(context.root().join("test.py")).expect("remove test file");

    assert_cmd_snapshot!(context.snapshot("prune"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Removed: <temp_dir>/snapshots/test__test_hello.snap (test file not found: test.py)

    1 snapshot(s) pruned.

    ----- stderr -----
    warning: Prune uses static analysis and may not detect all unreferenced snapshots.
    ");
}

#[test]
fn test_snapshot_prune_parametrized() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.parametrize('x', [1, 2])
def test_param(x):
    karva.assert_snapshot(str(x))
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    // Remove the parametrized test function
    context.write_file(
        "test.py",
        r"
import karva

def test_other():
    pass
        ",
    );

    assert_cmd_snapshot!(context.snapshot("prune"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Removed: <temp_dir>/snapshots/test__test_param(x=1).snap (function `test_param` not found in test.py)
    Removed: <temp_dir>/snapshots/test__test_param(x=2).snap (function `test_param` not found in test.py)

    2 snapshot(s) pruned.

    ----- stderr -----
    warning: Prune uses static analysis and may not detect all unreferenced snapshots.
    ");
}

#[test]
fn test_snapshot_prune_with_path_filter() {
    let context = TestContext::default();
    context.write_file(
        "test_one.py",
        r"
import karva

def test_from_one():
    karva.assert_snapshot('from file one')
        ",
    );
    context.write_file(
        "test_two.py",
        r"
import karva

def test_from_two():
    karva.assert_snapshot('from file two')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    // Remove both test functions
    context.write_file("test_one.py", "def test_other():\n    pass\n");
    context.write_file("test_two.py", "def test_other():\n    pass\n");

    // Only prune snapshots matching "snapshots/test_one"
    let mut prune_cmd = context.snapshot("prune");
    prune_cmd.arg("snapshots/test_one");
    assert_cmd_snapshot!(prune_cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Removed: <temp_dir>/snapshots/test_one__test_from_one.snap (function `test_from_one` not found in test_one.py)

    1 snapshot(s) pruned.

    ----- stderr -----
    warning: Prune uses static analysis and may not detect all unreferenced snapshots.
    ");

    // test_two snapshot should still exist
    assert!(
        context
            .root()
            .join("snapshots/test_two__test_from_two.snap")
            .exists(),
        "Expected test_two snapshot to still exist"
    );
}

#[test]
fn test_snapshot_prune_no_snapshots() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_hello():
    pass
        ",
    );

    assert_cmd_snapshot!(context.snapshot("prune"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    No unreferenced snapshots found.

    ----- stderr -----
    warning: Prune uses static analysis and may not detect all unreferenced snapshots.
    ");
}

#[test]
fn test_snapshot_delete_all() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    // Create .snap via --snapshot-update
    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    // Change test to create a .snap.new as well
    context.write_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('changed')
        ",
    );
    let _ = context.command_no_parallel().output();

    // Both .snap and .snap.new should exist
    assert!(
        context
            .root()
            .join("snapshots/test__test_hello.snap")
            .exists()
    );
    assert!(
        context
            .root()
            .join("snapshots/test__test_hello.snap.new")
            .exists()
    );

    assert_cmd_snapshot!(context.snapshot("delete"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Deleted: <temp_dir>/snapshots/test__test_hello.snap
    Deleted: <temp_dir>/snapshots/test__test_hello.snap.new

    2 snapshot file(s) deleted.

    ----- stderr -----
    ");

    assert!(
        !context
            .root()
            .join("snapshots/test__test_hello.snap")
            .exists()
    );
    assert!(
        !context
            .root()
            .join("snapshots/test__test_hello.snap.new")
            .exists()
    );
    assert!(!context.root().join("snapshots").exists());
}

#[test]
fn test_snapshot_delete_dry_run() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    let mut delete_cmd = context.snapshot("delete");
    delete_cmd.arg("--dry-run");
    assert_cmd_snapshot!(delete_cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Would delete: <temp_dir>/snapshots/test__test_hello.snap

    1 snapshot file(s) would be deleted.

    ----- stderr -----
    ");

    assert!(
        context
            .root()
            .join("snapshots/test__test_hello.snap")
            .exists(),
        "Expected snapshot to still exist after dry run"
    );
}

#[test]
fn test_snapshot_delete_no_snapshots() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_hello():
    pass
        ",
    );

    assert_cmd_snapshot!(context.snapshot("delete"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    No snapshot files found.

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_delete_with_path_filter() {
    let context = TestContext::default();
    context.write_file(
        "test_one.py",
        r"
import karva

def test_from_one():
    karva.assert_snapshot('from file one')
        ",
    );
    context.write_file(
        "test_two.py",
        r"
import karva

def test_from_two():
    karva.assert_snapshot('from file two')
        ",
    );

    let mut cmd = context.command_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    // Delete only snapshots matching "snapshots/test_one"
    let mut delete_cmd = context.snapshot("delete");
    delete_cmd.arg("snapshots/test_one");
    assert_cmd_snapshot!(delete_cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Deleted: <temp_dir>/snapshots/test_one__test_from_one.snap

    1 snapshot file(s) deleted.

    ----- stderr -----
    ");

    assert!(
        !context
            .root()
            .join("snapshots/test_one__test_from_one.snap")
            .exists(),
        "Expected test_one snapshot to be deleted"
    );
    assert!(
        context
            .root()
            .join("snapshots/test_two__test_from_two.snap")
            .exists(),
        "Expected test_two snapshot to still exist"
    );
}

#[test]
fn test_snapshot_delete_only_pending() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    // Run without --snapshot-update to create only .snap.new
    let _ = context.command_no_parallel().output();

    assert!(
        context
            .root()
            .join("snapshots/test__test_hello.snap.new")
            .exists()
    );
    assert!(
        !context
            .root()
            .join("snapshots/test__test_hello.snap")
            .exists()
    );

    assert_cmd_snapshot!(context.snapshot("delete"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Deleted: <temp_dir>/snapshots/test__test_hello.snap.new

    1 snapshot file(s) deleted.

    ----- stderr -----
    ");

    assert!(
        !context
            .root()
            .join("snapshots/test__test_hello.snap.new")
            .exists()
    );
    assert!(!context.root().join("snapshots").exists());
}
