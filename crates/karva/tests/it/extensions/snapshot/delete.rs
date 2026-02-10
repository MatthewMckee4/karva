use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

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

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    context.write_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('changed')
        ",
    );
    let _ = context.command_no_parallel().output();

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

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    assert_cmd_snapshot!(context.snapshot("delete").arg("--dry-run"), @r"
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

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    assert_cmd_snapshot!(context.snapshot("delete").arg("snapshots/test_one"), @r"
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
