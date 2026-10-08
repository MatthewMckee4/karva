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

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("a\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/1
    File: <temp_dir>/snapshots/test__test_hello.snap.new
    Source: test.py:5::test_hello

    ────────────┬[LONG-LINE]
              1 │ +hello world
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    review finished
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

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("r\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/1
    File: <temp_dir>/snapshots/test__test_hello.snap.new
    Source: test.py:5::test_hello

    ────────────┬[LONG-LINE]
              1 │ +hello world
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    review finished
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

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("s\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/1
    File: <temp_dir>/snapshots/test__test_hello.snap.new
    Source: test.py:5::test_hello

    ────────────┬[LONG-LINE]
              1 │ +hello world
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    review finished
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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            FAIL [TIME] test::test_one
            FAIL [TIME] test::test_two

    failures:

    test::test_one:

    error[test-failure]: Test `test_one` failed
     --> test.py:4:5
      |
    4 | def test_one():
      |     ^^^^^^^^
    info: Test failed here
     --> test.py:5:5
      |
    5 |     karva.assert_snapshot('first')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_one' in snapshots/test__test_one.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test::test_two:

    error[test-failure]: Test `test_two` failed
     --> test.py:7:5
      |
    7 | def test_two():
      |     ^^^^^^^^
    info: Test failed here
     --> test.py:8:5
      |
    8 |     karva.assert_snapshot('second')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_two' in snapshots/test__test_two.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("S\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/2
    File: <temp_dir>/snapshots/test__test_one.snap.new
    Source: test.py:5::test_one

    ────────────┬[LONG-LINE]
              1 │ +first
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    review finished
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
fn test_snapshot_review_with_source_file_filter() {
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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            FAIL [TIME] test_one::test_from_one
            FAIL [TIME] test_one_extra::test_from_extra

    failures:

    test_one::test_from_one:

    error[test-failure]: Test `test_from_one` failed
     --> test_one.py:4:5
      |
    4 | def test_from_one():
      |     ^^^^^^^^^^^^^
    info: Test failed here
     --> test_one.py:5:5
      |
    5 |     karva.assert_snapshot('from file one')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_from_one' in snapshots/test_one__test_from_one.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test_one_extra::test_from_extra:

    error[test-failure]: Test `test_from_extra` failed
     --> test_one_extra.py:4:5
      |
    4 | def test_from_extra():
      |     ^^^^^^^^^^^^^^^
    info: Test failed here
     --> test_one_extra.py:5:5
      |
    5 |     karva.assert_snapshot('from extra file')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_from_extra' in snapshots/test_one_extra__test_from_extra.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.snapshot("review").arg("test_one.py").pass_stdin("a\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/1
    File: <temp_dir>/snapshots/test_one__test_from_one.snap.new
    Source: test_one.py:5::test_from_one

    ────────────┬[LONG-LINE]
              1 │ +from file one
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    review finished
    accepted:
      <temp_dir>/snapshots/test_one__test_from_one.snap.new

    ----- stderr -----
    ");

    assert!(
        context
            .root()
            .join("snapshots/test_one__test_from_one.snap")
            .exists(),
        "Expected test_one snapshot to be accepted"
    );
    assert!(
        context
            .root()
            .join("snapshots/test_one_extra__test_from_extra.snap.new")
            .exists(),
        "Expected test_one_extra pending snapshot to still exist"
    );
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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            FAIL [TIME] test::test_alpha
            FAIL [TIME] test::test_beta

    failures:

    test::test_alpha:

    error[test-failure]: Test `test_alpha` failed
     --> test.py:4:5
      |
    4 | def test_alpha():
      |     ^^^^^^^^^^
    info: Test failed here
     --> test.py:5:5
      |
    5 |     karva.assert_snapshot('alpha')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_alpha' in snapshots/test__test_alpha.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test::test_beta:

    error[test-failure]: Test `test_beta` failed
     --> test.py:7:5
      |
    7 | def test_beta():
      |     ^^^^^^^^^
    info: Test failed here
     --> test.py:8:5
      |
    8 |     karva.assert_snapshot('beta')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_beta' in snapshots/test__test_beta.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("A\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/2
    File: <temp_dir>/snapshots/test__test_alpha.snap.new
    Source: test.py:5::test_alpha

    ────────────┬[LONG-LINE]
              1 │ +alpha
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    review finished
    accepted:
      <temp_dir>/snapshots/test__test_alpha.snap.new
      <temp_dir>/snapshots/test__test_beta.snap.new

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            FAIL [TIME] test::test_alpha
            FAIL [TIME] test::test_beta

    failures:

    test::test_alpha:

    error[test-failure]: Test `test_alpha` failed
     --> test.py:4:5
      |
    4 | def test_alpha():
      |     ^^^^^^^^^^
    info: Test failed here
     --> test.py:5:5
      |
    5 |     karva.assert_snapshot('alpha')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_alpha' in snapshots/test__test_alpha.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test::test_beta:

    error[test-failure]: Test `test_beta` failed
     --> test.py:7:5
      |
    7 | def test_beta():
      |     ^^^^^^^^^
    info: Test failed here
     --> test.py:8:5
      |
    8 |     karva.assert_snapshot('beta')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_beta' in snapshots/test__test_beta.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("R\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/2
    File: <temp_dir>/snapshots/test__test_alpha.snap.new
    Source: test.py:5::test_alpha

    ────────────┬[LONG-LINE]
              1 │ +alpha
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    review finished
    rejected:
      <temp_dir>/snapshots/test__test_alpha.snap.new
      <temp_dir>/snapshots/test__test_beta.snap.new

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            FAIL [TIME] test::test_alpha
            FAIL [TIME] test::test_beta

    failures:

    test::test_alpha:

    error[test-failure]: Test `test_alpha` failed
     --> test.py:4:5
      |
    4 | def test_alpha():
      |     ^^^^^^^^^^
    info: Test failed here
     --> test.py:5:5
      |
    5 |     karva.assert_snapshot('alpha')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_alpha' in snapshots/test__test_alpha.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test::test_beta:

    error[test-failure]: Test `test_beta` failed
     --> test.py:7:5
      |
    7 | def test_beta():
      |     ^^^^^^^^^
    info: Test failed here
     --> test.py:8:5
      |
    8 |     karva.assert_snapshot('beta')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New snapshot for 'test_beta' in snapshots/test__test_beta.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("a\nr\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/2
    File: <temp_dir>/snapshots/test__test_alpha.snap.new
    Source: test.py:5::test_alpha

    ────────────┬[LONG-LINE]
              1 │ +alpha
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    Snapshot 2/2
    File: <temp_dir>/snapshots/test__test_beta.snap.new
    Source: test.py:8::test_beta

    ────────────┬[LONG-LINE]
              1 │ +beta
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    review finished
    accepted:
      <temp_dir>/snapshots/test__test_alpha.snap.new
    rejected:
      <temp_dir>/snapshots/test__test_beta.snap.new

    ----- stderr -----
    ");

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
fn test_snapshot_review_shows_diff_for_mismatch() {
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
    karva.assert_snapshot('goodbye world')
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
    5 |     karva.assert_snapshot('goodbye world')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Snapshot mismatch for 'test_hello'.
     --> snapshots/test__test_hello.snap:4:1
      |
    4 - hello world
    4 + goodbye world
      |
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("a\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/1
    File: <temp_dir>/snapshots/test__test_hello.snap.new
    Source: test.py:5::test_hello

    ────────────┬[LONG-LINE]
        1       │ -hello world
              1 │ +goodbye world
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    review finished
    accepted:
      <temp_dir>/snapshots/test__test_hello.snap.new

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_hello.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_hello
    ---
    goodbye world
    ");
}
