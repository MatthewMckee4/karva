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

    assert_cmd_snapshot!(context
        .command_no_parallel()
        .arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_hello
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    context.write_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('changed')
        ",
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_hello

    failures:

    test::test_hello:

    error[test-failure]: Test `test_hello` failed
     --> test.py:4:5
      |
    4 | def test_hello():
      |     ^^^^^^^^^^
    info: Test failed here
     --> test.py:5:5
      |
    5 |     karva.assert_snapshot('changed')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Snapshot mismatch for 'test_hello'.
     --> snapshots/test__test_hello.snap:4:1
      |
    4 - hello world
    4 + changed
      |
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(context
        .command_no_parallel()
        .arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_hello
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(context
        .command_no_parallel()
        .arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_one::test_from_one
            PASS [TIME] test_two::test_from_two
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");

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
fn test_snapshot_delete_with_source_file_filter() {
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
        "test_one_extra.py",
        r"
import karva

def test_from_extra():
    karva.assert_snapshot('from extra file')
        ",
    );

    assert_cmd_snapshot!(context
        .command_no_parallel()
        .arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_one::test_from_one
            PASS [TIME] test_one_extra::test_from_extra
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.snapshot("delete").arg("test_one.py"), @r"
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
            .join("snapshots/test_one_extra__test_from_extra.snap")
            .exists(),
        "Expected test_one_extra snapshot to still exist"
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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_hello

    failures:

    test::test_hello:

    error[test-failure]: Test `test_hello` failed
     --> test.py:4:5
      |
    4 | def test_hello():
      |     ^^^^^^^^^^
    info: Test failed here
     --> test.py:5:5
      |
    5 |     karva.assert_snapshot('hello world')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_hello' in snapshots/test__test_hello.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");

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
