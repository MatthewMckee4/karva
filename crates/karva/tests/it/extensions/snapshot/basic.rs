use insta_cmd::assert_cmd_snapshot;
use karva_static::EnvVars;

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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_hello

    diagnostics:

    error[test-failure]: Test `test_hello` failed
     --> test.py:4:5
      |
    4 | def test_hello():
      |     ^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:5:5
      |
    5 |     karva.assert_snapshot('hello world')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: New snapshot for 'test_hello'.
          Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.
          Pending file: snapshots/test__test_hello.snap.new

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_hello
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

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
fn test_snapshot_update_env_creates_snap_file() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .env(EnvVars::KARVA_SNAPSHOT_UPDATE, "1"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_hello
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "
    );

    let content = context.read_file("snapshots/test__test_hello.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_hello
    ---
    hello world
    ");
}

#[test]
fn test_snapshot_update_false_overrides_env() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .env(EnvVars::KARVA_SNAPSHOT_UPDATE, "1")
            .arg("--snapshot-update=false"),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_hello

    diagnostics:

    error[test-failure]: Test `test_hello` failed
     --> test.py:4:5
      |
    4 | def test_hello():
      |     ^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:5:5
      |
    5 |     karva.assert_snapshot('hello world')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: New snapshot for 'test_hello'.
          Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.
          Pending file: snapshots/test__test_hello.snap.new

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );

    let content = context.read_file("snapshots/test__test_hello.snap.new");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_hello
    ---
    hello world
    ");
    assert!(
        !context
            .root()
            .join("snapshots/test__test_hello.snap")
            .exists()
    );
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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_hello
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_malformed_existing_file_fails_without_pending_snapshot() {
    let context = TestContext::with_files([
        (
            "test.py",
            r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
            ",
        ),
        ("snapshots/test__test_hello.snap", "not a snapshot"),
    ]);

    let output = context
        .command_no_parallel()
        .arg("--no-cache")
        .output()
        .expect("run karva");
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(!output.status.success());
    assert!(
        stdout.contains("Failed to read snapshot: malformed snapshot file")
            && stdout.contains("missing opening frontmatter separator"),
        "{stdout}"
    );
    assert!(
        !context
            .root()
            .join("snapshots/test__test_hello.snap.new")
            .exists()
    );
    assert_eq!(
        context.read_file("snapshots/test__test_hello.snap").trim(),
        "not a snapshot"
    );
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
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_hello

    diagnostics:

    error[test-failure]: Test `test_hello` failed
     --> test.py:4:5
      |
    4 | def test_hello():
      |     ^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:5:5
      |
    5 |     karva.assert_snapshot('goodbye world')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Snapshot mismatch for 'test_hello'.
          Snapshot file: snapshots/test__test_hello.snap
          ────────────┬───────────────────────────
              1       │ -hello world
                    1 │ +goodbye world
          ────────────┴───────────────────────────

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

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
    with karva.snapshot_settings(allow_duplicates=True):
        karva.assert_snapshot('first')
        karva.assert_snapshot('second')
        karva.assert_snapshot('third')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_multi
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let content_1 = context.read_file("snapshots/test__test_multi-0.snap");
    insta::assert_snapshot!(content_1, @r"
    ---
    source: test.py:6::test_multi
    ---
    first
    ");

    let content_2 = context.read_file("snapshots/test__test_multi-1.snap");
    insta::assert_snapshot!(content_2, @r"
    ---
    source: test.py:7::test_multi
    ---
    second
    ");

    let content_3 = context.read_file("snapshots/test__test_multi-2.snap");
    insta::assert_snapshot!(content_3, @r"
    ---
    source: test.py:8::test_multi
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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_param(x=1)
            PASS [TIME] test::test_param(x=2)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_overwrite
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

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

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_one
            FAIL [TIME] test::test_two

    diagnostics:

    error[test-failure]: Test `test_two` failed
     --> test.py:7:5
      |
    7 | def test_two():
      |     ^^^^^^^^
      |
    info: Test failed here
     --> test.py:8:5
      |
    8 |     karva.assert_snapshot('changed')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Snapshot mismatch for 'test_two'.
          Snapshot file: snapshots/test__test_two.snap
          ────────────┬───────────────────────────
              1       │ -second
                    1 │ +changed
          ────────────┴───────────────────────────

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

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
    with karva.snapshot_settings(allow_duplicates=True):
        karva.assert_snapshot('first')
        karva.assert_snapshot('named value', name='special')
        karva.assert_snapshot('third')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_mixed
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    assert!(
        context
            .root()
            .join("snapshots/test__test_mixed-0.snap")
            .exists(),
        "Expected first unnamed snapshot with -0 suffix"
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
            .join("snapshots/test__test_mixed-1.snap")
            .exists(),
        "Expected second unnamed snapshot with -1 suffix"
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

#[test]
fn test_snapshot_duplicate_unnamed_errors() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_multi():
    karva.assert_snapshot('first')
    karva.assert_snapshot('second')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_multi

    diagnostics:

    error[test-failure]: Test `test_multi` failed
     --> test.py:4:5
      |
    4 | def test_multi():
      |     ^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:6:5
      |
    6 |     karva.assert_snapshot('second')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Multiple unnamed snapshots in one test. Use 'name=' for each, or wrap in 'karva.snapshot_settings(allow_duplicates=True)'

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_duplicate_unnamed_with_allow_duplicates() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_multi():
    with karva.snapshot_settings(allow_duplicates=True):
        karva.assert_snapshot('first')
        karva.assert_snapshot('second')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_multi
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let content_1 = context.read_file("snapshots/test__test_multi-0.snap");
    insta::assert_snapshot!(content_1, @r"
    ---
    source: test.py:6::test_multi
    ---
    first
    ");

    let content_2 = context.read_file("snapshots/test__test_multi-1.snap");
    insta::assert_snapshot!(content_2, @r"
    ---
    source: test.py:7::test_multi
    ---
    second
    ");
}

#[test]
fn test_snapshot_in_subdirectory() {
    let context = TestContext::default();
    context.write_file(
        "sub/test_nested.py",
        r"
import karva

def test_in_sub():
    karva.assert_snapshot('sub value')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update").arg("sub/test_nested.py"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] sub.test_nested::test_in_sub
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let content = context.read_file("sub/snapshots/test_nested__test_in_sub.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test_nested.py:5::test_in_sub
    ---
    sub value
    ");
}

#[test]
fn test_snapshot_single_unnamed_gets_bare_name() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_single():
    karva.assert_snapshot('only one')
        ",
    );

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    assert!(
        context
            .root()
            .join("snapshots/test__test_single.snap")
            .exists(),
        "Expected bare snapshot name without numeric suffix"
    );
    assert!(
        !context
            .root()
            .join("snapshots/test__test_single-0.snap")
            .exists(),
        "Should NOT have -0 suffix for single unnamed snapshot"
    );
}
