use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_json_snapshot_basic() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_data():
    karva.assert_json_snapshot({"b": 2, "a": 1})
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_data ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_data.snap");
    insta::assert_snapshot!(content, @r#"
    ---
    source: test.py:5::test_data
    ---
    {
      "a": 1,
      "b": 2
    }
    "#);
}

#[test]
fn test_json_snapshot_nested_data() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_nested():
    data = {"users": [{"name": "Alice", "age": 30}, {"name": "Bob", "age": 25}], "count": 2}
    karva.assert_json_snapshot(data)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_nested ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_nested.snap");
    insta::assert_snapshot!(content, @r#"
    ---
    source: test.py:6::test_nested
    ---
    {
      "count": 2,
      "users": [
        {
          "age": 30,
          "name": "Alice"
        },
        {
          "age": 25,
          "name": "Bob"
        }
      ]
    }
    "#);
}

#[test]
fn test_json_snapshot_update_mode() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_json():
    karva.assert_json_snapshot({"key": "value"})
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_json ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    assert!(
        context
            .root()
            .join("snapshots/test__test_json.snap")
            .exists(),
        "Expected .snap file to be created"
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_json ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_json_snapshot_inline() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_inline_json():
    karva.assert_json_snapshot({"a": 1}, inline='{\n  "a": 1\n}')
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_inline_json ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_json_snapshot_with_filters() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_json_filtered():
    with karva.snapshot_settings(filters=[
        (r"\d{4}-\d{2}-\d{2}", "[date]"),
    ]):
        karva.assert_json_snapshot({"event": "created", "date": "2024-01-15"})
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_json_filtered ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_json_filtered.snap");
    insta::assert_snapshot!(content, @r#"
    ---
    source: test.py:8::test_json_filtered
    ---
    {
      "date": "[date]",
      "event": "created"
    }
    "#);
}

#[test]
fn test_json_snapshot_non_serializable() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_not_json():
    karva.assert_json_snapshot(object())
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_not_json ... FAILED

    diagnostics:

    error[test-failure]: Test `test_not_json` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_not_json():
      |     ^^^^^^^^^^^^^
    5 |     karva.assert_json_snapshot(object())
      |
    info: Test failed here
     --> test.py:5:5
      |
    4 | def test_not_json():
    5 |     karva.assert_json_snapshot(object())
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: assert_json_snapshot() value is not JSON serializable

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
