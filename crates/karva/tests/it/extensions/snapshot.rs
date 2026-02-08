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
    assert_cmd_snapshot!(context.test_no_parallel(), @r"
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
    info: New snapshot for 'test::test_hello'.
    Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.
    Pending file: <temp_dir>/snapshots/test__test__test_hello.snap.new

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    // Verify .snap.new file was created
    let snap_new_path = context
        .root()
        .join("snapshots/test__test__test_hello.snap.new");
    assert!(snap_new_path.exists(), "Expected .snap.new file to exist");
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

    let mut cmd = context.test_no_parallel();
    cmd.arg("--snapshot-update");

    assert_cmd_snapshot!(cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_hello ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    // Verify .snap file was created
    let snap_path = context.root().join("snapshots/test__test__test_hello.snap");
    assert!(snap_path.exists(), "Expected .snap file to exist");
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
    let mut cmd = context.test_no_parallel();
    cmd.arg("--snapshot-update");
    let _ = cmd.output();

    // Second run should match
    assert_cmd_snapshot!(context.test_no_parallel(), @r"
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
    let mut cmd = context.test_no_parallel();
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
    assert_cmd_snapshot!(context.test_no_parallel(), @r"
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
    info: Snapshot mismatch for 'test::test_hello'.
    Snapshot file: <temp_dir>/snapshots/test__test__test_hello.snap

    -hello world
    +goodbye world


    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_named() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello', name='greeting')
        ",
    );

    let mut cmd = context.test_no_parallel();
    cmd.arg("--snapshot-update");

    assert_cmd_snapshot!(cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_hello ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let snap_path = context
        .root()
        .join("snapshots/test__test__test_hello--greeting.snap");
    assert!(snap_path.exists(), "Expected named snapshot file to exist");
}

#[test]
fn test_snapshot_format_repr() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot({'a': 1}, format='repr')
        ",
    );

    let mut cmd = context.test_no_parallel();
    cmd.arg("--snapshot-update");

    assert_cmd_snapshot!(cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_hello ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let snap_path = context.root().join("snapshots/test__test__test_hello.snap");
    let content = std::fs::read_to_string(&snap_path).expect("read snap file");
    assert!(
        content.contains("{'a': 1}"),
        "Expected repr format in snapshot, got: {content}"
    );
}

#[test]
fn test_snapshot_format_json() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot({'a': 1, 'b': 2}, format='json')
        ",
    );

    let mut cmd = context.test_no_parallel();
    cmd.arg("--snapshot-update");

    assert_cmd_snapshot!(cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_hello ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let snap_path = context.root().join("snapshots/test__test__test_hello.snap");
    let content = std::fs::read_to_string(&snap_path).expect("read snap file");
    assert!(
        content.contains(r#""a": 1"#),
        "Expected JSON format in snapshot, got: {content}"
    );
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

    let mut cmd = context.test_no_parallel();
    cmd.arg("--snapshot-update");

    assert_cmd_snapshot!(cmd, @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_multi ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    // First snapshot: test_multi (no suffix)
    assert!(
        context
            .root()
            .join("snapshots/test__test__test_multi.snap")
            .exists()
    );
    // Second snapshot: test_multi-2
    assert!(
        context
            .root()
            .join("snapshots/test__test__test_multi-2.snap")
            .exists()
    );
    // Third snapshot: test_multi-3
    assert!(
        context
            .root()
            .join("snapshots/test__test__test_multi-3.snap")
            .exists()
    );
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
    let _ = context.test_no_parallel().output();

    // Run accept command
    assert_cmd_snapshot!(context.snapshot("accept"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Accepted: <temp_dir>/snapshots/test__test__test_hello.snap.new

    1 snapshot(s) accepted.

    ----- stderr -----
    ");

    // Verify .snap file now exists and .snap.new is gone
    let snap_path = context.root().join("snapshots/test__test__test_hello.snap");
    let snap_new_path = context
        .root()
        .join("snapshots/test__test__test_hello.snap.new");
    assert!(
        snap_path.exists(),
        "Expected .snap file to exist after accept"
    );
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
    let _ = context.test_no_parallel().output();

    // Run reject command
    assert_cmd_snapshot!(context.snapshot("reject"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Rejected: <temp_dir>/snapshots/test__test__test_hello.snap.new

    1 snapshot(s) rejected.

    ----- stderr -----
    ");

    // Verify .snap.new is gone and no .snap was created
    let snap_path = context.root().join("snapshots/test__test__test_hello.snap");
    let snap_new_path = context
        .root()
        .join("snapshots/test__test__test_hello.snap.new");
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
    let _ = context.test_no_parallel().output();

    // Run pending command
    assert_cmd_snapshot!(context.snapshot("pending"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    <temp_dir>/snapshots/test__test__test_hello.snap.new

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

    let mut cmd = context.test_no_parallel();
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

    // Check that parametrized snapshot files exist
    assert!(
        context
            .root()
            .join("snapshots/test__test__test_param(x=1).snap")
            .exists()
    );
}
