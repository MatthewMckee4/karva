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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_hello ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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
    test test::test_hello ... FAILED

    diagnostics:

    error[test-failure]: Test `test_hello` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_hello():
      |     ^^^^^^^^^^
    5 |     karva.assert_snapshot("goodbye", inline="hello")
      |
    info: Test failed here
     --> test.py:5:5
      |
    4 | def test_hello():
    5 |     karva.assert_snapshot("goodbye", inline="hello")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Inline snapshot mismatch for 'test_hello'.
          [LONG-LINE]┬[LONG-LINE]
              1       | -hello
                    1 | +goodbye
          [LONG-LINE]┴[LONG-LINE]

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_hello ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_lines ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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
    karva.assert_snapshot(\"line 1\\nline 2\", inline=\"\"\"\\\n        line 1\n        line 2\n    \"\"\")\n",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_lines ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_inline_snapshot_multiple_per_test() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_multi():
    karva.assert_snapshot("first", inline="")
    karva.assert_snapshot("second", inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_multi ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let source = context.read_file("test.py");
    insta::assert_snapshot!(source, @r#"
    import karva

    def test_multi():
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

    let _ = context.command_no_parallel().output();

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

    let _ = context.command_no_parallel().output();

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
fn test_inline_snapshot_pending() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_hello():
    karva.assert_snapshot("hello world", inline="")
        "#,
    );

    let _ = context.command_no_parallel().output();

    assert_cmd_snapshot!(context.snapshot("pending"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    <temp_dir>/snapshots/test__test_hello_inline_5.snap.new

    1 pending snapshot(s).

    ----- stderr -----
    ");
}
