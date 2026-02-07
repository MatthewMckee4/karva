use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

const TWO_TESTS: &str = r"
def test_alpha():
    assert True

def test_beta():
    assert True
";

#[test]
fn name_filter_substring_match() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg("alpha"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_alpha ... ok
    test test::test_beta ... skipped

    test result: ok. 1 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn name_filter_anchored_regex() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg("beta$"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_alpha ... skipped
    test test::test_beta ... ok

    test result: ok. 1 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn name_filter_multiple_flags_or_semantics() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg("alpha").arg("-m").arg("beta"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_alpha ... ok
    test test::test_beta ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn name_filter_no_matches() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg("nonexistent"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_alpha ... skipped
    test test::test_beta ... skipped

    test result: ok. 0 passed; 0 failed; 2 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn name_filter_invalid_regex() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg("[invalid"), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: invalid regex pattern `[invalid`: regex parse error:
        [invalid
        ^
    error: unclosed character class
      Cause: regex parse error:
        [invalid
        ^
    error: unclosed character class
    ");
}

#[test]
fn name_filter_parametrize() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.parametrize('x', [1, 2, 3])
def test_param(x):
    assert x > 0

def test_other():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg("test_param"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_param(x=1) ... ok
    test test::test_param(x=2) ... ok
    test test::test_param(x=3) ... ok
    test test::test_other ... skipped

    test result: ok. 3 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn name_filter_match_all() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg(".*"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_alpha ... ok
    test test::test_beta ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
