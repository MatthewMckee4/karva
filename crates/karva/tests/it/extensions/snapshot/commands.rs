use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

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

    let _ = context.command_no_parallel().output();

    assert_cmd_snapshot!(context.snapshot("accept"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Accepted: <temp_dir>/snapshots/test__test_hello.snap.new

    1 snapshot(s) accepted.

    ----- stderr -----
    ");

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

    let _ = context.command_no_parallel().output();

    assert_cmd_snapshot!(context.snapshot("reject"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Rejected: <temp_dir>/snapshots/test__test_hello.snap.new

    1 snapshot(s) rejected.

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
fn test_snapshot_pending_command() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    let _ = context.command_no_parallel().output();

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
