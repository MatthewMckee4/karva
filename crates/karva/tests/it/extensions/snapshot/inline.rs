use std::io::Write;
use std::process::Stdio;

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
          ────────────┬───────────────────────────
              1       │ -hello
                    1 │ +goodbye
          ────────────┴───────────────────────────

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
    karva.assert_snapshot(\"line 1\\nline 2\", inline=\"\"\"\\\n        line 1\n        line 2\n        \"\"\")\n",
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
fn test_inline_snapshot_multiline_closing_indent() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_closing():
    karva.assert_snapshot("line 1\nline 2", inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_closing ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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
fn test_inline_snapshot_with_backslash() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_backslash():
    karva.assert_snapshot("path\\to\\file", inline="")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_backslash ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_quotes ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    "#);

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

    let _ = context.command_no_parallel().output();

    let mut child = context
        .snapshot("review")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn review");
    child
        .stdin
        .take()
        .expect("no stdin")
        .write_all(b"a\ns\n")
        .expect("write failed");
    let _ = child.wait_with_output();

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

    let mut child = context
        .snapshot("review")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn review");
    child
        .stdin
        .take()
        .expect("no stdin")
        .write_all(b"a\n")
        .expect("write failed");
    let _ = child.wait_with_output();

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

    let _ = context.command_no_parallel().output();

    let mut child = context
        .snapshot("review")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn review");
    child
        .stdin
        .take()
        .expect("no stdin")
        .write_all(b"a\ns\n")
        .expect("write failed");
    let _ = child.wait_with_output();

    let source = context.read_file("test.py");
    assert!(
        source.contains(r#"inline="hello""#),
        "Expected first inline rewritten, got:\n{source}"
    );

    let _ = context.command_no_parallel().output();

    let output = context.snapshot("accept").output().expect("accept failed");
    assert!(output.status.success(), "Expected accept to succeed");

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

    let _ = context.command_no_parallel().output();

    let mut child = context
        .snapshot("review")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn review");
    child
        .stdin
        .take()
        .expect("no stdin")
        .write_all(b"a\ns\n")
        .expect("write failed");
    let _ = child.wait_with_output();

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

    let mut child = context
        .snapshot("review")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn review");
    child
        .stdin
        .take()
        .expect("no stdin")
        .write_all(b"a\n")
        .expect("write failed");
    let _ = child.wait_with_output();

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

    let _ = context.command_no_parallel().output();

    let mut child = context
        .snapshot("review")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn review");
    child
        .stdin
        .take()
        .expect("no stdin")
        .write_all(b"a\ns\n")
        .expect("write failed");
    let _ = child.wait_with_output();

    let old_pending = context
        .root()
        .join("snapshots/test__test_second_inline_8.snap.new");
    assert!(old_pending.exists(), "Expected old .snap.new at line 8");

    // Re-run: second test now fails at shifted line 12, creating a second .snap.new
    let _ = context.command_no_parallel().output();

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

    let output = context.snapshot("accept").output().expect("accept failed");
    assert!(output.status.success(), "Expected accept to succeed");

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

    let _ = context.command_no_parallel().output();

    let output = context.snapshot("accept").output().expect("accept failed");
    assert!(output.status.success(), "Expected accept to succeed");

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

    let _ = context.command_no_parallel().output();

    let mut child = context
        .snapshot("review")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn review");
    child
        .stdin
        .take()
        .expect("no stdin")
        .write_all(b"a\ns\n")
        .expect("write failed");
    let _ = child.wait_with_output();

    let mut child = context
        .snapshot("review")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("failed to spawn review");
    child
        .stdin
        .take()
        .expect("no stdin")
        .write_all(b"a\n")
        .expect("write failed");
    let _ = child.wait_with_output();

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

    let _ = context.command_no_parallel().output();

    let output = context.snapshot("accept").output().expect("accept failed");
    assert!(output.status.success(), "Expected accept to succeed");

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

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_a ... ok
    test test::test_b ... ok
    test test::test_c ... ok
    test test::test_d ... ok
    test test::test_e ... ok
    test test::test_f ... ok

    test result: ok. 6 passed; 0 failed; 0 skipped; finished in [TIME]

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

    let _ = context.command_no_parallel().output();

    let output = context.snapshot("accept").output().expect("accept failed");
    assert!(output.status.success(), "Expected accept to succeed");

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

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_a ... ok
    test test::test_b ... ok
    test test::test_c ... ok
    test test::test_d ... ok
    test test::test_e ... ok
    test test::test_f ... ok

    test result: ok. 6 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_a ... ok
    test test::test_b ... ok
    test test::test_c ... ok
    test test::test_d ... ok
    test test::test_e ... ok
    test test::test_f ... ok

    test result: ok. 6 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
