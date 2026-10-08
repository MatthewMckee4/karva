use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

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
fn test_inline_snapshot_mismatch_no_update() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_hello():
    karva.assert_snapshot("goodbye", inline="hello")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
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
    5 |     karva.assert_snapshot("goodbye", inline="hello")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Inline snapshot mismatch for 'test_hello'.
    1 │ -hello
    1 │ +goodbye
    info: Re-run with `--snapshot-update` to accept.

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_lines
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

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
    karva.assert_snapshot(\"line 1\\nline 2\", inline=\"\"\"\\\n        line 1\n        line 2\n        \"\"\")\n",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_lines
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_inline_snapshot_multiline_closing_indent() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_closing():
    karva.assert_snapshot("line 1\nline 2", inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_closing
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    assert!(
        source.contains("        \"\"\""),
        "Expected closing triple-quote at 8-space indent (content level), got:\n{source}"
    );
    assert!(
        !source.contains("\n    \"\"\""),
        "Closing triple-quote should NOT be at 4-space indent (call level), got:\n{source}"
    );
}

#[test]
fn test_inline_snapshot_multiple_per_test() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_multi():
    with karva.snapshot_settings(allow_duplicates=True):
        karva.assert_snapshot("first", inline="")
        karva.assert_snapshot("second", inline="")
        "#,
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

    let source = context.read_file("test.py");
    insta::assert_snapshot!(source, @r#"
    import karva

    def test_multi():
        with karva.snapshot_settings(allow_duplicates=True):
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

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
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
    5 |     karva.assert_snapshot("hello world", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_hello' in snapshots/test__test_hello_inline_5.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);

    let source_before = context.read_file("test.py");
    assert!(
        source_before.contains(r#"inline="""#),
        "Expected source to still have empty inline"
    );

    assert_cmd_snapshot!(context.snapshot("accept"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Accepted: <temp_dir>/snapshots/test__test_hello_inline_5.snap.new

    1 snapshot(s) accepted.

    ----- stderr -----
    ");

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

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
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
    5 |     karva.assert_snapshot("hello world", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_hello' in snapshots/test__test_hello_inline_5.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);

    assert_cmd_snapshot!(context.snapshot("reject"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    Rejected: <temp_dir>/snapshots/test__test_hello_inline_5.snap.new

    1 snapshot(s) rejected.

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    insta::assert_snapshot!(source, @r#"
    import karva

    def test_hello():
        karva.assert_snapshot("hello world", inline="")
    "#);
}

#[test]
fn test_inline_snapshot_with_backslash() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_backslash():
    karva.assert_snapshot("path\\to\\file", inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_backslash
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    insta::assert_snapshot!(source, @r#"

    import karva

    def test_backslash():
        karva.assert_snapshot("path\/to\/file", inline="path\/to\/file")
    "#);
}

#[test]
fn test_inline_snapshot_with_quotes() {
    let context = TestContext::with_file(
        "test.py",
        "
import karva

def test_quotes():
    karva.assert_snapshot('say \"hi\"', inline=\"\")
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_quotes
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    assert!(
        source.contains("say \\\"hi\\\""),
        "Expected escaped double quotes in inline value, got: {source}"
    );
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

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
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
    5 |     karva.assert_snapshot("hello world", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_hello' in snapshots/test__test_hello_inline_5.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);

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
fn test_inline_review_accept_first_then_review_accept_second() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_first():
    karva.assert_snapshot("hello", inline="")

def test_second():
    karva.assert_snapshot("world", inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
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
    5 |     karva.assert_snapshot("hello", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_first' in snapshots/test__test_first_inline_5.snap.new.
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
    8 |     karva.assert_snapshot("world", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_second' in snapshots/test__test_second_inline_8.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    "#);

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("a\ns\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/2
    File: <temp_dir>/snapshots/test__test_first_inline_5.snap.new
    Source: test.py:5::test_first

    ────────────┬[LONG-LINE]
              1 │ +hello
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    Snapshot 2/2
    File: <temp_dir>/snapshots/test__test_second_inline_8.snap.new
    Source: test.py:8::test_second

    ────────────┬[LONG-LINE]
              1 │ +world
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
      <temp_dir>/snapshots/test__test_first_inline_5.snap.new
    skipped:
      <temp_dir>/snapshots/test__test_second_inline_8.snap.new

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    assert!(
        source.contains(r#"inline="hello""#),
        "Expected first inline rewritten to 'hello', got:\n{source}"
    );
    assert!(
        source.contains(r#"karva.assert_snapshot("world", inline="")"#),
        "Expected second inline still empty, got:\n{source}"
    );

    let pending = context
        .root()
        .join("snapshots/test__test_second_inline_8.snap.new");
    assert!(pending.exists(), "Expected second .snap.new to still exist");

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("a\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/1
    File: <temp_dir>/snapshots/test__test_second_inline_8.snap.new
    Source: test.py:8::test_second

    ────────────┬[LONG-LINE]
              1 │ +world
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
      <temp_dir>/snapshots/test__test_second_inline_8.snap.new

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    assert!(
        source.contains(r#"inline="hello""#),
        "Expected first inline still 'hello', got:\n{source}"
    );
    assert!(
        source.contains(r#"inline="world""#),
        "Expected second inline rewritten to 'world', got:\n{source}"
    );
    assert!(
        !context
            .root()
            .join("snapshots/test__test_first_inline_5.snap.new")
            .exists(),
        "Expected no pending first snapshot"
    );
    assert!(
        !context
            .root()
            .join("snapshots/test__test_second_inline_8.snap.new")
            .exists(),
        "Expected no pending second snapshot"
    );
}

#[test]
fn test_inline_review_accept_first_then_rerun_accept_second() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_first():
    karva.assert_snapshot("hello", inline="")

def test_second():
    karva.assert_snapshot("world", inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
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
    5 |     karva.assert_snapshot("hello", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_first' in snapshots/test__test_first_inline_5.snap.new.
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
    8 |     karva.assert_snapshot("world", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_second' in snapshots/test__test_second_inline_8.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    "#);

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("a\ns\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/2
    File: <temp_dir>/snapshots/test__test_first_inline_5.snap.new
    Source: test.py:5::test_first

    ────────────┬[LONG-LINE]
              1 │ +hello
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    Snapshot 2/2
    File: <temp_dir>/snapshots/test__test_second_inline_8.snap.new
    Source: test.py:8::test_second

    ────────────┬[LONG-LINE]
              1 │ +world
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
      <temp_dir>/snapshots/test__test_first_inline_5.snap.new
    skipped:
      <temp_dir>/snapshots/test__test_second_inline_8.snap.new

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    assert!(
        source.contains(r#"inline="hello""#),
        "Expected first inline rewritten, got:\n{source}"
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_first
            FAIL [TIME] test::test_second

    failures:

    test::test_second:

    error[test-failure]: Test `test_second` failed
     --> test.py:7:5
      |
    7 | def test_second():
      |     ^^^^^^^^^^^
    info: Test failed here
     --> test.py:8:5
      |
    8 |     karva.assert_snapshot("world", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_second' in snapshots/test__test_second_inline_8.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);

    assert_cmd_snapshot!(context.snapshot("accept"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    Accepted: <temp_dir>/snapshots/test__test_second_inline_8.snap.new

    1 snapshot(s) accepted.

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    assert!(
        source.contains(r#"inline="hello""#),
        "Expected first inline still correct, got:\n{source}"
    );
    assert!(
        source.contains(r#"inline="world""#),
        "Expected second inline rewritten to 'world', got:\n{source}"
    );
    assert!(
        !context
            .root()
            .join("snapshots/test__test_second_inline_8.snap.new")
            .exists(),
        "Expected no pending second snapshot"
    );
}

#[test]
fn test_inline_accept_multiline_shifts_lines() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_first():
    karva.assert_snapshot("line1\nline2\nline3", inline="")

def test_second():
    karva.assert_snapshot("world", inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
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
    5 |     karva.assert_snapshot("line1/nline2/nline3", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_first' in snapshots/test__test_first_inline_5.snap.new.
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
    8 |     karva.assert_snapshot("world", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_second' in snapshots/test__test_second_inline_8.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    "#);

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("a\ns\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/2
    File: <temp_dir>/snapshots/test__test_first_inline_5.snap.new
    Source: test.py:5::test_first

    ────────────┬[LONG-LINE]
              1 │ +line1
              2 │ +line2
              3 │ +line3
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    Snapshot 2/2
    File: <temp_dir>/snapshots/test__test_second_inline_8.snap.new
    Source: test.py:8::test_second

    ────────────┬[LONG-LINE]
              1 │ +world
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
      <temp_dir>/snapshots/test__test_first_inline_5.snap.new
    skipped:
      <temp_dir>/snapshots/test__test_second_inline_8.snap.new

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    assert!(
        source.contains("inline=\"\"\""),
        "Expected first inline rewritten to triple-quoted, got:\n{source}"
    );
    assert!(
        source.contains(r#"karva.assert_snapshot("world", inline="")"#),
        "Expected second inline still empty, got:\n{source}"
    );

    let pending = context
        .root()
        .join("snapshots/test__test_second_inline_8.snap.new");
    assert!(pending.exists(), "Expected second .snap.new to still exist");

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("a\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/1
    File: <temp_dir>/snapshots/test__test_second_inline_8.snap.new
    Source: test.py:8::test_second

    ────────────┬[LONG-LINE]
              1 │ +world
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
      <temp_dir>/snapshots/test__test_second_inline_8.snap.new

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    assert!(
        source.contains(r#"inline="world""#),
        "Expected second inline rewritten to 'world', got:\n{source}"
    );
    assert!(!pending.exists(), "Expected no pending second snapshot");
}

#[test]
fn test_inline_multiline_accept_rerun_duplicate_pending() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_first():
    karva.assert_snapshot("line1\nline2\nline3", inline="")

def test_second():
    karva.assert_snapshot("world", inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
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
    5 |     karva.assert_snapshot("line1/nline2/nline3", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_first' in snapshots/test__test_first_inline_5.snap.new.
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
    8 |     karva.assert_snapshot("world", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_second' in snapshots/test__test_second_inline_8.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    "#);

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("a\ns\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/2
    File: <temp_dir>/snapshots/test__test_first_inline_5.snap.new
    Source: test.py:5::test_first

    ────────────┬[LONG-LINE]
              1 │ +line1
              2 │ +line2
              3 │ +line3
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    Snapshot 2/2
    File: <temp_dir>/snapshots/test__test_second_inline_8.snap.new
    Source: test.py:8::test_second

    ────────────┬[LONG-LINE]
              1 │ +world
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
      <temp_dir>/snapshots/test__test_first_inline_5.snap.new
    skipped:
      <temp_dir>/snapshots/test__test_second_inline_8.snap.new

    ----- stderr -----
    ");

    let old_pending = context
        .root()
        .join("snapshots/test__test_second_inline_8.snap.new");
    assert!(old_pending.exists(), "Expected old .snap.new at line 8");

    // Re-run: second test now fails at shifted line 12, creating a second .snap.new
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_first
            FAIL [TIME] test::test_second

    failures:

    test::test_second:

    error[test-failure]: Test `test_second` failed
      --> test.py:11:5
       |
    11 | def test_second():
       |     ^^^^^^^^^^^
    info: Test failed here
      --> test.py:12:5
       |
    12 |     karva.assert_snapshot("world", inline="")
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_second' in snapshots/test__test_second_inline_12.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);

    let new_pending = context
        .root()
        .join("snapshots/test__test_second_inline_12.snap.new");
    assert!(
        new_pending.exists(),
        "Expected new .snap.new at shifted line 12"
    );
    assert!(
        old_pending.exists(),
        "Expected old .snap.new at line 8 to still exist"
    );

    assert_cmd_snapshot!(context.snapshot("accept"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    Accepted: <temp_dir>/snapshots/test__test_second_inline_12.snap.new
    Accepted: <temp_dir>/snapshots/test__test_second_inline_8.snap.new

    2 snapshot(s) accepted.

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    assert!(
        source.contains(r#"inline="world""#),
        "Expected second inline rewritten to 'world', got:\n{source}"
    );
    assert!(!old_pending.exists(), "Expected old .snap.new removed");
    assert!(!new_pending.exists(), "Expected new .snap.new removed");
}

/// Accepting a multiline inline shifts line numbers. `test_third`'s `.snap.new`
/// has a stale line that lands before `test_middle` — `find_inline_argument` must
/// skip `test_middle`'s call and find `test_third`'s.
#[test]
fn test_inline_multiline_accept_does_not_corrupt_intervening_inline() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_first():
    karva.assert_snapshot("a\nb\nc", inline="")

def test_middle():
    karva.assert_snapshot("fixed", inline="fixed")

def test_third():
    karva.assert_snapshot("hello", inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 3 tests across 1 worker
            FAIL [TIME] test::test_first
            PASS [TIME] test::test_middle
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
    5 |     karva.assert_snapshot("a/nb/nc", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_first' in snapshots/test__test_first_inline_5.snap.new.
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
    11 |     karva.assert_snapshot("hello", inline="")
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_third' in snapshots/test__test_third_inline_11.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 3 tests run: 1 passed, 2 failed, 0 skipped

    ----- stderr -----
    "#);

    assert_cmd_snapshot!(context.snapshot("accept"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    Accepted: <temp_dir>/snapshots/test__test_first_inline_5.snap.new
    Accepted: <temp_dir>/snapshots/test__test_third_inline_11.snap.new

    2 snapshot(s) accepted.

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    assert!(
        source.contains(r#"karva.assert_snapshot("fixed", inline="fixed")"#),
        "Middle inline was corrupted! Got:\n{source}"
    );
    assert!(
        source.contains(r#"karva.assert_snapshot("hello", inline="hello")"#),
        "Third inline not rewritten correctly! Got:\n{source}"
    );
}

/// Same corruption scenario as above, but via review (accept first, skip third,
/// then review again to accept third with stale line number).
#[test]
fn test_inline_multiline_review_does_not_corrupt_intervening_inline() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_first():
    karva.assert_snapshot("a\nb\nc", inline="")

def test_middle():
    karva.assert_snapshot("fixed", inline="fixed")

def test_third():
    karva.assert_snapshot("hello", inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 3 tests across 1 worker
            FAIL [TIME] test::test_first
            PASS [TIME] test::test_middle
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
    5 |     karva.assert_snapshot("a/nb/nc", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_first' in snapshots/test__test_first_inline_5.snap.new.
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
    11 |     karva.assert_snapshot("hello", inline="")
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_third' in snapshots/test__test_third_inline_11.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 3 tests run: 1 passed, 2 failed, 0 skipped

    ----- stderr -----
    "#);

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("a\ns\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/2
    File: <temp_dir>/snapshots/test__test_first_inline_5.snap.new
    Source: test.py:5::test_first

    ────────────┬[LONG-LINE]
              1 │ +a
              2 │ +b
              3 │ +c
    ────────────┴[LONG-LINE]

      a accept     keep the new snapshot
      r reject     retain the old snapshot
      s skip       keep both for now
      i hide info  toggles extended snapshot info
      d hide diff  toggle snapshot diff

      Tip: Use uppercase A/R/S to apply to all remaining snapshots
    >
    Snapshot 2/2
    File: <temp_dir>/snapshots/test__test_third_inline_11.snap.new
    Source: test.py:11::test_third

    ────────────┬[LONG-LINE]
              1 │ +hello
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
      <temp_dir>/snapshots/test__test_first_inline_5.snap.new
    skipped:
      <temp_dir>/snapshots/test__test_third_inline_11.snap.new

    ----- stderr -----
    ");

    assert_cmd_snapshot!(context.snapshot("review").pass_stdin("a\n"), @"
    success: true
    exit_code: 0
    ----- stdout -----

    Snapshot 1/1
    File: <temp_dir>/snapshots/test__test_third_inline_11.snap.new
    Source: test.py:11::test_third

    ────────────┬[LONG-LINE]
              1 │ +hello
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
      <temp_dir>/snapshots/test__test_third_inline_11.snap.new

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    assert!(
        source.contains(r#"karva.assert_snapshot("fixed", inline="fixed")"#),
        "Middle inline was corrupted by review! Got:\n{source}"
    );
    assert!(
        source.contains(r#"karva.assert_snapshot("hello", inline="hello")"#),
        "Third inline not rewritten by review! Got:\n{source}"
    );
}

/// Batch-accept 6 multiline inline snapshots in a single file.
/// Each empty inline expands to a triple-quoted multiline literal, shifting
/// line numbers for subsequent calls. Without reverse-order batch processing,
/// later accepts would use stale line numbers and corrupt the file.
#[test]
fn test_inline_batch_accept_many_multiline() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_a():
    karva.assert_snapshot("a1\na2", inline="")

def test_b():
    karva.assert_snapshot("b1\nb2", inline="")

def test_c():
    karva.assert_snapshot("c1\nc2", inline="")

def test_d():
    karva.assert_snapshot("d1\nd2", inline="")

def test_e():
    karva.assert_snapshot("e1\ne2", inline="")

def test_f():
    karva.assert_snapshot("f1\nf2", inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 6 tests across 1 worker
            FAIL [TIME] test::test_a
            FAIL [TIME] test::test_b
            FAIL [TIME] test::test_c
            FAIL [TIME] test::test_d
            FAIL [TIME] test::test_e
            FAIL [TIME] test::test_f

    failures:

    test::test_a:

    error[test-failure]: Test `test_a` failed
     --> test.py:4:5
      |
    4 | def test_a():
      |     ^^^^^^
    info: Test failed here
     --> test.py:5:5
      |
    5 |     karva.assert_snapshot("a1/na2", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_a' in snapshots/test__test_a_inline_5.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test::test_b:

    error[test-failure]: Test `test_b` failed
     --> test.py:7:5
      |
    7 | def test_b():
      |     ^^^^^^
    info: Test failed here
     --> test.py:8:5
      |
    8 |     karva.assert_snapshot("b1/nb2", inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_b' in snapshots/test__test_b_inline_8.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test::test_c:

    error[test-failure]: Test `test_c` failed
      --> test.py:10:5
       |
    10 | def test_c():
       |     ^^^^^^
    info: Test failed here
      --> test.py:11:5
       |
    11 |     karva.assert_snapshot("c1/nc2", inline="")
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_c' in snapshots/test__test_c_inline_11.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test::test_d:

    error[test-failure]: Test `test_d` failed
      --> test.py:13:5
       |
    13 | def test_d():
       |     ^^^^^^
    info: Test failed here
      --> test.py:14:5
       |
    14 |     karva.assert_snapshot("d1/nd2", inline="")
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_d' in snapshots/test__test_d_inline_14.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test::test_e:

    error[test-failure]: Test `test_e` failed
      --> test.py:16:5
       |
    16 | def test_e():
       |     ^^^^^^
    info: Test failed here
      --> test.py:17:5
       |
    17 |     karva.assert_snapshot("e1/ne2", inline="")
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_e' in snapshots/test__test_e_inline_17.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    test::test_f:

    error[test-failure]: Test `test_f` failed
      --> test.py:19:5
       |
    19 | def test_f():
       |     ^^^^^^
    info: Test failed here
      --> test.py:20:5
       |
    20 |     karva.assert_snapshot("f1/nf2", inline="")
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_f' in snapshots/test__test_f_inline_20.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 6 tests run: 0 passed, 6 failed, 0 skipped

    ----- stderr -----
    "#);

    assert_cmd_snapshot!(context.snapshot("accept"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    Accepted: <temp_dir>/snapshots/test__test_a_inline_5.snap.new
    Accepted: <temp_dir>/snapshots/test__test_b_inline_8.snap.new
    Accepted: <temp_dir>/snapshots/test__test_c_inline_11.snap.new
    Accepted: <temp_dir>/snapshots/test__test_d_inline_14.snap.new
    Accepted: <temp_dir>/snapshots/test__test_e_inline_17.snap.new
    Accepted: <temp_dir>/snapshots/test__test_f_inline_20.snap.new

    6 snapshot(s) accepted.

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    insta::assert_snapshot!(source, @r#"

    import karva

    def test_a():
        karva.assert_snapshot("a1/na2", inline="""/
            a1
            a2
            """)

    def test_b():
        karva.assert_snapshot("b1/nb2", inline="""/
            b1
            b2
            """)

    def test_c():
        karva.assert_snapshot("c1/nc2", inline="""/
            c1
            c2
            """)

    def test_d():
        karva.assert_snapshot("d1/nd2", inline="""/
            d1
            d2
            """)

    def test_e():
        karva.assert_snapshot("e1/ne2", inline="""/
            e1
            e2
            """)

    def test_f():
        karva.assert_snapshot("f1/nf2", inline="""/
            f1
            f2
            """)
    "#);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 6 tests across 1 worker
            PASS [TIME] test::test_a
            PASS [TIME] test::test_b
            PASS [TIME] test::test_c
            PASS [TIME] test::test_d
            PASS [TIME] test::test_e
            PASS [TIME] test::test_f
    ────────────
         Summary [TIME] 6 tests run: 6 passed, 0 skipped

    ----- stderr -----
    ");
}

/// Inner class definitions (e.g. `def __repr__`) must not confuse the
/// function-name verification that prevents cross-function corruption.
#[test]
fn test_inline_snapshot_with_inner_class_def() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_custom():
    class Custom:
        def __repr__(self) -> str:
            return "CustomRepr(x=1)"

    karva.assert_snapshot(repr(Custom()), inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_custom

    failures:

    test::test_custom:

    error[test-failure]: Test `test_custom` failed
     --> test.py:4:5
      |
    4 | def test_custom():
      |     ^^^^^^^^^^^
    info: Test failed here
     --> test.py:9:5
      |
    9 |     karva.assert_snapshot(repr(Custom()), inline="")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: New inline snapshot for 'test_custom' in snapshots/test__test_custom_inline_9.snap.new.
    info: Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);

    assert_cmd_snapshot!(context.snapshot("accept"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    Accepted: <temp_dir>/snapshots/test__test_custom_inline_9.snap.new

    1 snapshot(s) accepted.

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    insta::assert_snapshot!(source, @r#"
    import karva

    def test_custom():
        class Custom:
            def __repr__(self) -> str:
                return "CustomRepr(x=1)"

        karva.assert_snapshot(repr(Custom()), inline="CustomRepr(x=1)")
    "#);
}

#[test]
fn test_inline_snapshot_single_quoted_argument() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_single_quote():
    karva.assert_snapshot('hello', inline='')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_single_quote
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    assert!(
        source.contains("inline='hello'") || source.contains(r#"inline="hello""#),
        "Expected inline value rewritten, got:\n{source}"
    );
}

#[test]
fn test_inline_snapshot_empty_string_value() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_empty():
    karva.assert_snapshot("", inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_empty
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    insta::assert_snapshot!(source, @r#"
    import karva

    def test_empty():
        karva.assert_snapshot("", inline="")
    "#);
}

/// Same as batch accept, but via `--snapshot-update` which rewrites inline
/// snapshots during test execution rather than via a separate accept step.
#[test]
fn test_inline_batch_update_many_multiline() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_a():
    karva.assert_snapshot("a1\na2", inline="")

def test_b():
    karva.assert_snapshot("b1\nb2", inline="")

def test_c():
    karva.assert_snapshot("c1\nc2", inline="")

def test_d():
    karva.assert_snapshot("d1\nd2", inline="")

def test_e():
    karva.assert_snapshot("e1\ne2", inline="")

def test_f():
    karva.assert_snapshot("f1\nf2", inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 6 tests across 1 worker
            PASS [TIME] test::test_a
            PASS [TIME] test::test_b
            PASS [TIME] test::test_c
            PASS [TIME] test::test_d
            PASS [TIME] test::test_e
            PASS [TIME] test::test_f
    ────────────
         Summary [TIME] 6 tests run: 6 passed, 0 skipped

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    insta::assert_snapshot!(source, @r#"

    import karva

    def test_a():
        karva.assert_snapshot("a1/na2", inline="""/
            a1
            a2
            """)

    def test_b():
        karva.assert_snapshot("b1/nb2", inline="""/
            b1
            b2
            """)

    def test_c():
        karva.assert_snapshot("c1/nc2", inline="""/
            c1
            c2
            """)

    def test_d():
        karva.assert_snapshot("d1/nd2", inline="""/
            d1
            d2
            """)

    def test_e():
        karva.assert_snapshot("e1/ne2", inline="""/
            e1
            e2
            """)

    def test_f():
        karva.assert_snapshot("f1/nf2", inline="""/
            f1
            f2
            """)
    "#);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 6 tests across 1 worker
            PASS [TIME] test::test_a
            PASS [TIME] test::test_b
            PASS [TIME] test::test_c
            PASS [TIME] test::test_d
            PASS [TIME] test::test_e
            PASS [TIME] test::test_f
    ────────────
         Summary [TIME] 6 tests run: 6 passed, 0 skipped

    ----- stderr -----
    ");
}
