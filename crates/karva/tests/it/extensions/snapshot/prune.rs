use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

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

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

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

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    context.write_file(
        "test.py",
        r"
import karva

def test_other():
    pass
        ",
    );

    assert_cmd_snapshot!(context.snapshot("prune").arg("--dry-run"), @r"
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

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

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

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

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

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

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

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    context.write_file("test_one.py", "def test_other():\n    pass\n");
    context.write_file("test_two.py", "def test_other():\n    pass\n");

    assert_cmd_snapshot!(context.snapshot("prune").arg("snapshots/test_one"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Removed: <temp_dir>/snapshots/test_one__test_from_one.snap (function `test_from_one` not found in test_one.py)

    1 snapshot(s) pruned.

    ----- stderr -----
    warning: Prune uses static analysis and may not detect all unreferenced snapshots.
    ");

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
fn test_snapshot_named_prune() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world', name='greeting')
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

def test_other():
    pass
        ",
    );

    assert_cmd_snapshot!(context.snapshot("prune"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Removed: <temp_dir>/snapshots/test__test_hello--greeting.snap (function `test_hello` not found in test.py)

    1 snapshot(s) pruned.

    ----- stderr -----
    warning: Prune uses static analysis and may not detect all unreferenced snapshots.
    ");

    assert!(
        !context
            .root()
            .join("snapshots/test__test_hello--greeting.snap")
            .exists(),
        "Expected named snapshot to be removed"
    );
}
