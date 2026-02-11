use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_snapshot_multiline_content() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_lines():
    karva.assert_snapshot('line one\nline two\nline three')
        ",
    );

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    let content = context.read_file("snapshots/test__test_lines.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_lines
    ---
    line one
    line two
    line three
    ");
}

#[test]
fn test_snapshot_content_with_leading_trailing_spaces() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_spaces():
    karva.assert_snapshot('  hello  ')
        ",
    );

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    let content = context.read_file("snapshots/test__test_spaces.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_spaces
    ---
      hello
    ");
}

#[test]
fn test_snapshot_content_with_tabs_and_mixed_whitespace() {
    let context = TestContext::with_file(
        "test.py",
        "
import karva

def test_tabs():
    karva.assert_snapshot('col1\\tcol2\\tcol3\\n  indented\\n\\ttab indented')
        ",
    );

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    let content = context.read_file("snapshots/test__test_tabs.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_tabs
    ---
    col1	col2	col3
      indented
    	tab indented
    ");
}

#[test]
fn test_snapshot_empty_string() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_empty():
    karva.assert_snapshot('')
        ",
    );

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    let content = context.read_file("snapshots/test__test_empty.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_empty
    ---

    ");
}

#[test]
fn test_snapshot_unicode_content() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_unicode():
    karva.assert_snapshot('Hello éèê ☃ ❤ üñîçödé')
        ",
    );

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    let content = context.read_file("snapshots/test__test_unicode.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_unicode
    ---
    Hello éèê ☃ ❤ üñîçödé
    ");
}

#[test]
fn test_snapshot_content_with_special_characters() {
    let context = TestContext::with_file(
        "test.py",
        "
import karva

def test_special():
    karva.assert_snapshot('angle <brackets> & ampersand\\n\"double quotes\"\\n$dollar @at #hash')
        ",
    );

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    let content = context.read_file("snapshots/test__test_special.snap");
    insta::assert_snapshot!(content, @r#"
    ---
    source: test.py:5::test_special
    ---
    angle <brackets> & ampersand
    "double quotes"
    $dollar @at #hash
    "#);
}

#[test]
fn test_snapshot_content_with_dashes_like_yaml() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_dashes():
    karva.assert_snapshot('---\nthis looks like yaml\n---')
        ",
    );

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    let content = context.read_file("snapshots/test__test_dashes.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_dashes
    ---
    ---
    this looks like yaml
    ---
    ");
}

#[test]
fn test_snapshot_multiline_mismatch() {
    let context = TestContext::with_file(
        "test.py",
        "
import karva

def test_poem():
    karva.assert_snapshot('roses are red\\nviolets are blue')
        ",
    );

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

    context.write_file(
        "test.py",
        "
import karva

def test_poem():
    karva.assert_snapshot('roses are red\\nviolets are purple\\nsugar is sweet')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_poem ... FAILED

    diagnostics:

    error[test-failure]: Test `test_poem` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_poem():
      |     ^^^^^^^^^
    5 |     karva.assert_snapshot('roses are red/nviolets are purple/nsugar is sweet')
      |
    info: Test failed here
     --> test.py:5:5
      |
    4 | def test_poem():
    5 |     karva.assert_snapshot('roses are red/nviolets are purple/nsugar is sweet')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Snapshot mismatch for 'test_poem'.
          Snapshot file: <temp_dir>/snapshots/test__test_poem.snap
          ────────────┬───────────────────────────
              1     1 │  roses are red
              2       │ -violets are blue
                    2 │ +violets are purple
                    3 │ +sugar is sweet
          ────────────┴───────────────────────────

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
