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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 3 tests across 1 worker
            FAIL [TIME] test::test_first
            FAIL [TIME] test::test_second
            FAIL [TIME] test::test_third

    failures:

    test::test_first:

    error[test-failure]: Test `test_first` failed
     --> test.py:4:5
      |
    4 | def test_first():
      |     ^^^^^^^^^^
    info: Test failed here
     --> test.py:5:5
      |
    5 |     karva.assert_snapshot('aaa')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_first' in snapshots/test__test_first.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test::test_second:

    error[test-failure]: Test `test_second` failed
     --> test.py:7:5
      |
    7 | def test_second():
      |     ^^^^^^^^^^^
    info: Test failed here
     --> test.py:8:5
      |
    8 |     karva.assert_snapshot('bbb')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_second' in snapshots/test__test_second.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test::test_third:

    error[test-failure]: Test `test_third` failed
      --> test.py:10:5
       |
    10 | def test_third():
       |     ^^^^^^^^^^
    info: Test failed here
      --> test.py:11:5
       |
    11 |     karva.assert_snapshot('ccc')
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_third' in snapshots/test__test_third.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 3 tests run: 0 passed, 3 failed, 0 skipped

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_first
            PASS [TIME] test::test_second
            PASS [TIME] test::test_third
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            FAIL [TIME] test::test_first
            FAIL [TIME] test::test_second

    failures:

    test::test_first:

    error[test-failure]: Test `test_first` failed
     --> test.py:4:5
      |
    4 | def test_first():
      |     ^^^^^^^^^^
    info: Test failed here
     --> test.py:5:5
      |
    5 |     karva.assert_snapshot('aaa')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_first' in snapshots/test__test_first.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test::test_second:

    error[test-failure]: Test `test_second` failed
     --> test.py:7:5
      |
    7 | def test_second():
      |     ^^^^^^^^^^^
    info: Test failed here
     --> test.py:8:5
      |
    8 |     karva.assert_snapshot('bbb')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_second' in snapshots/test__test_second.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    ");

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
fn test_snapshot_accept_no_pending() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_pass():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_pass
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.snapshot("accept"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    No pending snapshots found.

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_reject_no_pending() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_pass():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_pass
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.snapshot("reject"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    No pending snapshots found.

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_pending_none() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_pass():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_pass
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.snapshot("pending"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    No pending snapshots found.

    ----- stderr -----
    ");
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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 3 tests across 1 worker
            FAIL [TIME] test::test_aaa
            FAIL [TIME] test::test_bbb
            FAIL [TIME] test::test_ccc

    failures:

    test::test_aaa:

    error[test-failure]: Test `test_aaa` failed
     --> test.py:4:5
      |
    4 | def test_aaa():
      |     ^^^^^^^^
    info: Test failed here
     --> test.py:5:5
      |
    5 |     karva.assert_snapshot('aaa')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_aaa' in snapshots/test__test_aaa.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test::test_bbb:

    error[test-failure]: Test `test_bbb` failed
     --> test.py:7:5
      |
    7 | def test_bbb():
      |     ^^^^^^^^
    info: Test failed here
     --> test.py:8:5
      |
    8 |     karva.assert_snapshot('bbb')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_bbb' in snapshots/test__test_bbb.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test::test_ccc:

    error[test-failure]: Test `test_ccc` failed
      --> test.py:10:5
       |
    10 | def test_ccc():
       |     ^^^^^^^^
    info: Test failed here
      --> test.py:11:5
       |
    11 |     karva.assert_snapshot('ccc')
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_ccc' in snapshots/test__test_ccc.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 3 tests run: 0 passed, 3 failed, 0 skipped

    ----- stderr -----
    ");

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
