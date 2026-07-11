use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_snapshot_filter_basic() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_filtered():
    with karva.snapshot_settings(filters=[
        (r"\d{4}-\d{2}-\d{2}", "[date]"),
    ]):
        karva.assert_snapshot("created on 2024-01-15")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_filtered
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_filtered.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:8::test_filtered
    ---
    created on [date]
    ");
}

#[test]
fn test_snapshot_filter_multiple() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_multi_filter():
    with karva.snapshot_settings(filters=[
        (r"\d{4}-\d{2}-\d{2}", "[date]"),
        (r"[0-9a-f-]{36}", "[uuid]"),
    ]):
        karva.assert_snapshot("id=550e8400-e29b-41d4-a716-446655440000 date=2024-01-15")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_multi_filter
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_multi_filter.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:9::test_multi_filter
    ---
    id=[uuid] date=[date]
    ");
}

#[test]
fn test_snapshot_filter_no_match() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_no_match():
    with karva.snapshot_settings(filters=[
        (r"ZZZZZ", "[never]"),
    ]):
        karva.assert_snapshot("hello world")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_no_match
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_no_match.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:8::test_no_match
    ---
    hello world
    ");
}

#[test]
fn test_snapshot_filter_replacement_is_literal() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_literal_replacement():
    with karva.snapshot_settings(filters=[
        (r"cost=\d+", "cost=$1"),
    ]):
        karva.assert_snapshot("cost=42")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_literal_replacement
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_literal_replacement.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:8::test_literal_replacement
    ---
    cost=$1
    ");
}

#[test]
fn test_snapshot_filter_invalid_regex() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_bad_regex():
    with karva.snapshot_settings(filters=[
        (r"(unclosed", "[bad]"),
    ]):
        karva.assert_snapshot("hello")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_bad_regex

    diagnostics:

    error[test-failure]: Test `test_bad_regex` failed
     --> test.py:4:5
      |
    4 | def test_bad_regex():
      |     ^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:8:9
      |
    8 |         karva.assert_snapshot("hello")
      |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Invalid regex pattern in snapshot filter `(unclosed`: regex parse error:
              (unclosed
              ^
          error: unclosed group

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn test_snapshot_filter_nested_settings() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_nested():
    with karva.snapshot_settings(filters=[(r"\d+ms", "[duration]")]):
        with karva.snapshot_settings(filters=[(r"/tmp/\S+", "[path]")]):
            karva.assert_snapshot("took 42ms at /tmp/foo")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_nested
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_nested.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:7::test_nested
    ---
    took [duration] at [path]
    ");
}

#[test]
fn test_snapshot_filter_with_inline() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_inline_filtered():
    with karva.snapshot_settings(filters=[(r"\d{4}-\d{2}-\d{2}", "[date]")]):
        karva.assert_snapshot("date is 2024-01-15", inline="date is [date]")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_inline_filtered
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_filter_with_update() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_update_filtered():
    with karva.snapshot_settings(filters=[(r"\d{4}-\d{2}-\d{2}", "[date]")]):
        karva.assert_snapshot("created 2024-06-15")
        "#,
    );

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    let content = context.read_file("snapshots/test__test_update_filtered.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:6::test_update_filtered
    ---
    created [date]
    ");

    context.write_file(
        "test.py",
        r#"
import karva

def test_update_filtered():
    with karva.snapshot_settings(filters=[(r"\d{4}-\d{2}-\d{2}", "[date]")]):
        karva.assert_snapshot("created 2025-12-25")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_update_filtered
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_filter_empty_list() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_empty_filters():
    with karva.snapshot_settings(filters=[]):
        karva.assert_snapshot("unchanged")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_empty_filters
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_empty_filters.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:6::test_empty_filters
    ---
    unchanged
    ");
}
