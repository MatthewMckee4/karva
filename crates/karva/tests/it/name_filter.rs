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

#[test]
fn name_filter_alternation() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_login():
    assert True

def test_logout():
    assert True

def test_signup():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg("login|signup"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_login ... ok
    test test::test_logout ... skipped
    test test::test_signup ... ok

    test result: ok. 2 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn name_filter_character_class() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_v1():
    assert True

def test_v2():
    assert True

def test_v10():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg(r"test_v[12]$"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_v1 ... ok
    test test::test_v2 ... ok
    test test::test_v10 ... skipped

    test result: ok. 2 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn name_filter_quantifier() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_a():
    assert True

def test_ab():
    assert True

def test_abb():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg(r"test_ab+$"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_a ... skipped
    test test::test_ab ... ok
    test test::test_abb ... ok

    test result: ok. 2 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn name_filter_qualified_name_prefix() {
    let context = TestContext::with_files([
        (
            "test_auth.py",
            r"
def test_login():
    assert True

def test_logout():
    assert True
            ",
        ),
        (
            "test_users.py",
            r"
def test_login():
    assert True
            ",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg("^test_auth::"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_users::test_login ... skipped
    test test_auth::test_login ... ok
    test test_auth::test_logout ... ok

    test result: ok. 2 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn name_filter_combined_with_tag_filter() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
def test_slow_alpha():
    assert True

@karva.tags.slow
def test_slow_beta():
    assert True

def test_fast_alpha():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-t").arg("slow").arg("-m").arg("alpha"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_slow_alpha ... ok
    test test::test_slow_beta ... skipped
    test test::test_fast_alpha ... skipped

    test result: ok. 1 passed; 0 failed; 2 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn name_filter_case_sensitive() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_Alpha():
    assert True

def test_alpha():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg("Alpha"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_Alpha ... ok
    test test::test_alpha ... skipped

    test result: ok. 1 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn name_filter_case_insensitive_regex() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_Alpha():
    assert True

def test_alpha():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg("(?i)alpha"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_Alpha ... ok
    test test::test_alpha ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn name_filter_dot_metacharacter() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_a1():
    assert True

def test_a2():
    assert True

def test_ab():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg(r"test_a\d"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_a1 ... ok
    test test::test_a2 ... ok
    test test::test_ab ... skipped

    test result: ok. 2 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

// ── Invalid regex error cases ────────────────────────────────────────

#[test]
fn name_filter_invalid_regex_unclosed_group() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg("(unclosed"), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: invalid regex pattern `(unclosed`: regex parse error:
        (unclosed
        ^
    error: unclosed group
    ");
}

#[test]
fn name_filter_invalid_regex_invalid_repetition() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg("*invalid"), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: invalid regex pattern `*invalid`: regex parse error:
        *invalid
        ^
    error: repetition operator missing expression
    ");
}

#[test]
fn name_filter_invalid_regex_bad_escape() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-m").arg(r"\p{Invalid}"), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: invalid regex pattern `\p{Invalid}`: regex parse error:
        \p{Invalid}
        ^^^^^^^^^^^
    error: Unicode property not found
    ");
}
