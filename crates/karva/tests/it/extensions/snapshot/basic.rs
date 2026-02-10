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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
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

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

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

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    context.write_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('goodbye world')
        ",
    );

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
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
fn test_snapshot_update_overwrites_existing() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_overwrite():
    karva.assert_snapshot('original')
        ",
    );

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    let content = context.read_file("snapshots/test__test_overwrite.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_overwrite
    ---
    original
    ");

    context.write_file(
        "test.py",
        r"
import karva

def test_overwrite():
    karva.assert_snapshot('updated')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
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

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

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
fn test_snapshot_named_and_unnamed_counter_gap() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_mixed():
    karva.assert_snapshot('first')
    karva.assert_snapshot('named value', name='special')
    karva.assert_snapshot('third')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_mixed ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    assert!(
        context
            .root()
            .join("snapshots/test__test_mixed.snap")
            .exists(),
        "Expected first unnamed snapshot"
    );
    assert!(
        context
            .root()
            .join("snapshots/test__test_mixed--special.snap")
            .exists(),
        "Expected named snapshot"
    );
    assert!(
        context
            .root()
            .join("snapshots/test__test_mixed-3.snap")
            .exists(),
        "Expected third snapshot with -3 suffix"
    );
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

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

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
