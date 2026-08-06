use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_accept_then_unchanged_source_passes() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    let _ = context.command_no_parallel().output();
    let _ = context.snapshot("accept").output();

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
fn test_accept_then_modify_source_fails() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world')
        ",
    );

    let _ = context.command_no_parallel().output();
    let _ = context.snapshot("accept").output();

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
    4 | def test_hello():
      |     ^^^^^^^^^^
    info: Test failed here
     --> test.py:5:5
    5 |     karva.assert_snapshot('goodbye world')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Snapshot mismatch for 'test_hello'.
          Snapshot file: snapshots/test__test_hello.snap
          ────────────┬───────────────────────────
              1       │ -hello world
                    1 │ +goodbye world
          ────────────┴───────────────────────────
          Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.
          Pending file: snapshots/test__test_hello.snap.new

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_accept_json_then_add_role_fails() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def get_user_data():
    return {"id": 1, "name": "Alice", "roles": ["admin", "user"]}

def test_user_data():
    result = get_user_data()
    karva.assert_json_snapshot(result)
        "#,
    );

    let _ = context.command_no_parallel().output();
    let _ = context.snapshot("accept").output();

    let content = context.read_file("snapshots/test__test_user_data.snap");
    insta::assert_snapshot!(content, @r#"
    ---
    source: test.py:9::test_user_data
    ---
    {
      "id": 1,
      "name": "Alice",
      "roles": [
        "admin",
        "user"
      ]
    }
    "#);

    context.write_file(
        "test.py",
        r#"
import karva

def get_user_data():
    return {"id": 1, "name": "Alice", "roles": ["admin", "user", "hr"]}

def test_user_data():
    result = get_user_data()
    karva.assert_json_snapshot(result)
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_user_data

    failures:

    test::test_user_data:

    error[test-failure]: Test `test_user_data` failed
     --> test.py:7:5
    7 | def test_user_data():
      |     ^^^^^^^^^^^^^^
    info: Test failed here
     --> test.py:9:5
    9 |     karva.assert_json_snapshot(result)
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Snapshot mismatch for 'test_user_data'.
          Snapshot file: snapshots/test__test_user_data.snap
          ────────────┬───────────────────────────
              2     2 │    "id": 1,
              3     3 │    "name": "Alice",
              4     4 │    "roles": [
              5     5 │      "admin",
              6       │ -    "user"
                    6 │ +    "user",
                    7 │ +    "hr"
              7     8 │    ]
              8     9 │  }
          ────────────┴───────────────────────────
          Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.
          Pending file: snapshots/test__test_user_data.snap.new

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn test_accept_json_then_add_role_then_accept_again() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def get_user_data():
    return {"id": 1, "name": "Alice", "roles": ["admin", "user"]}

def test_user_data():
    result = get_user_data()
    karva.assert_json_snapshot(result)
        "#,
    );

    let _ = context.command_no_parallel().output();
    let _ = context.snapshot("accept").output();

    context.write_file(
        "test.py",
        r#"
import karva

def get_user_data():
    return {"id": 1, "name": "Alice", "roles": ["admin", "user", "hr"]}

def test_user_data():
    result = get_user_data()
    karva.assert_json_snapshot(result)
        "#,
    );

    let _ = context.command_no_parallel().output();
    let _ = context.snapshot("accept").output();

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_user_data
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_user_data.snap");
    insta::assert_snapshot!(content, @r#"
    ---
    source: test.py:9::test_user_data
    ---
    {
      "id": 1,
      "name": "Alice",
      "roles": [
        "admin",
        "user",
        "hr"
      ]
    }
    "#);
}

#[test]
fn test_accept_then_update_with_snapshot_update_overwrites() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('first')
        ",
    );

    let _ = context.command_no_parallel().output();
    let _ = context.snapshot("accept").output();

    context.write_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('second')
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
    insta::assert_snapshot!(content, @"
    ---
    source: test.py:5::test_hello
    ---
    second
    ");
}

#[test]
fn test_accept_multiple_then_modify_one_fails_only_one() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_first():
    karva.assert_json_snapshot({"value": 1})

def test_second():
    karva.assert_json_snapshot({"value": 2})
        "#,
    );

    let _ = context.command_no_parallel().output();
    let _ = context.snapshot("accept").output();

    context.write_file(
        "test.py",
        r#"
import karva

def test_first():
    karva.assert_json_snapshot({"value": 1})

def test_second():
    karva.assert_json_snapshot({"value": 99})
        "#,
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
    7 | def test_second():
      |     ^^^^^^^^^^^
    info: Test failed here
     --> test.py:8:5
    8 |     karva.assert_json_snapshot({"value": 99})
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Snapshot mismatch for 'test_second'.
          Snapshot file: snapshots/test__test_second.snap
          ────────────┬───────────────────────────
              1     1 │  {
              2       │ -  "value": 2
                    2 │ +  "value": 99
              3     3 │  }
          ────────────┴───────────────────────────
          Run `karva snapshot accept` to accept, or re-run with `--snapshot-update`.
          Pending file: snapshots/test__test_second.snap.new

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}
