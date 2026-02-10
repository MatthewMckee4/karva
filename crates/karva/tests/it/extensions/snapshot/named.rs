use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_snapshot_named_creates_correct_file() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world', name='greeting')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_hello ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let content = context.read_file("snapshots/test__test_hello--greeting.snap");
    insta::assert_snapshot!(content, @r"
    ---
    source: test.py:5::test_hello
    ---
    hello world
    ");
}

#[test]
fn test_snapshot_named_matches() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world', name='greeting')
        ",
    );

    let _ = context
        .command_no_parallel()
        .arg("--snapshot-update")
        .output();

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
fn test_snapshot_named_mismatch() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_hello():
    karva.assert_snapshot('hello world', name='greeting')
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
    karva.assert_snapshot('goodbye world', name='greeting')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
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
    5 |     karva.assert_snapshot('goodbye world', name='greeting')
      |
    info: Test failed here
     --> test.py:5:5
      |
    4 | def test_hello():
    5 |     karva.assert_snapshot('goodbye world', name='greeting')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Snapshot mismatch for 'test_hello--greeting'.
          Snapshot file: <temp_dir>/snapshots/test__test_hello--greeting.snap
          [LONG-LINE]┬[LONG-LINE]
              1       | -hello world
                    1 | +goodbye world
          [LONG-LINE]┴[LONG-LINE]

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_named_multiple() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_page():
    karva.assert_snapshot('Welcome', name='header')
    karva.assert_snapshot('Goodbye', name='footer')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_page ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let header = context.read_file("snapshots/test__test_page--header.snap");
    insta::assert_snapshot!(header, @r"
    ---
    source: test.py:5::test_page
    ---
    Welcome
    ");

    let footer = context.read_file("snapshots/test__test_page--footer.snap");
    insta::assert_snapshot!(footer, @r"
    ---
    source: test.py:6::test_page
    ---
    Goodbye
    ");
}

#[test]
fn test_snapshot_named_and_unnamed_mixed() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_mixed():
    karva.assert_snapshot('unnamed value')
    karva.assert_snapshot('named value', name='special')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_mixed ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let unnamed = context.read_file("snapshots/test__test_mixed.snap");
    insta::assert_snapshot!(unnamed, @r"
    ---
    source: test.py:5::test_mixed
    ---
    unnamed value
    ");

    let named = context.read_file("snapshots/test__test_mixed--special.snap");
    insta::assert_snapshot!(named, @r"
    ---
    source: test.py:6::test_mixed
    ---
    named value
    ");
}

#[test]
fn test_snapshot_name_and_inline_error() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_both():
    karva.assert_snapshot('value', name='foo', inline='bar')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_both ... FAILED

    diagnostics:

    error[test-failure]: Test `test_both` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_both():
      |     ^^^^^^^^^
    5 |     karva.assert_snapshot('value', name='foo', inline='bar')
      |
    info: Test failed here
     --> test.py:5:5
      |
    4 | def test_both():
    5 |     karva.assert_snapshot('value', name='foo', inline='bar')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: assert_snapshot() cannot use both 'inline' and 'name' arguments

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn test_snapshot_named_parametrized() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.parametrize('lang', ['en', 'fr'])
def test_translate(lang):
    karva.assert_snapshot(f'hello_{lang}', name='greeting')
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--snapshot-update"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_translate(lang=en) ... ok
    test test::test_translate(lang=fr) ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");

    let en = context.read_file("snapshots/test__test_translate--greeting(lang=en).snap");
    insta::assert_snapshot!(en, @r"
    ---
    source: test.py:6::test_translate(lang=en)
    ---
    hello_en
    ");

    let fr = context.read_file("snapshots/test__test_translate--greeting(lang=fr).snap");
    insta::assert_snapshot!(fr, @r"
    ---
    source: test.py:6::test_translate(lang=fr)
    ---
    hello_fr
    ");
}
