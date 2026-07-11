use insta_cmd::assert_cmd_snapshot;
use karva_static::EnvVars;

use crate::common::TestContext;

const TWO_TESTS: &str = r"
def test_alpha():
    assert True

def test_beta():
    assert True
";

const NO_TESTS: &str = r"
def helper():
    pass
";

#[test]
fn filterset_test_substring() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("test(~alpha)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_alpha
    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_test_regex_anchored() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("test(/beta$/)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_beta
    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_test_multiple_flags_or_semantics() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("-E")
            .arg("test(~alpha)")
            .arg("-E")
            .arg("test(~beta)"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_alpha
            PASS [TIME] test::test_beta
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn filterset_test_no_matches() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("test(~nonexistent)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 skipped

    ----- stderr -----
    ");
}

#[test]
fn no_tests_collected_default_fails() {
    let context = TestContext::with_file("test.py", NO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped
    error: no tests to run
    (hint: use `--no-tests` to customize)

    ----- stderr -----
    ");
}

#[test]
fn no_tests_collected_auto_with_filter_passes() {
    let context = TestContext::with_file("test.py", NO_TESTS);
    assert_cmd_snapshot!(
        context.command_no_parallel().arg("-E").arg("test(~helper)"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn no_tests_collected_pass() {
    let context = TestContext::with_file("test.py", NO_TESTS);
    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--no-tests").arg("pass"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn no_tests_collected_warn() {
    let context = TestContext::with_file("test.py", NO_TESTS);
    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--no-tests").arg("warn"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped
    warning: no tests to run

    ----- stderr -----
    "
    );
}

#[test]
fn no_tests_collected_env_warn() {
    let context = TestContext::with_file("test.py", NO_TESTS);
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .env(EnvVars::KARVA_NO_TESTS, "warn"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped
    warning: no tests to run

    ----- stderr -----
    "
    );
}

#[test]
fn no_tests_collected_cli_overrides_env() {
    let context = TestContext::with_file("test.py", NO_TESTS);
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .env(EnvVars::KARVA_NO_TESTS, "pass")
            .arg("--no-tests")
            .arg("fail"),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped
    error: no tests to run
    (hint: use `--no-tests` to customize)

    ----- stderr -----
    "
    );
}

#[test]
fn no_tests_collected_fail_explicit() {
    let context = TestContext::with_file("test.py", NO_TESTS);
    assert_cmd_snapshot!(
        context.command_no_parallel().arg("--no-tests").arg("fail"),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped
    error: no tests to run
    (hint: use `--no-tests` to customize)

    ----- stderr -----
    "
    );
}

#[test]
fn filterset_test_parametrize() {
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

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("test(~test_param)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_param(x=1)
            PASS [TIME] test::test_param(x=2)
            PASS [TIME] test::test_param(x=3)
    ────────────
         Summary [TIME] 4 tests run: 3 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
#[cfg(unix)]
fn filterset_test_regex_match_all() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("test(/.*/)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_alpha
            PASS [TIME] test::test_beta
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_test_regex_alternation() {
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

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("test(/login|signup/)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_login
            PASS [TIME] test::test_signup
    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_test_regex_character_class() {
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

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg(r"test(/test_v[12]$/)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_v1
            PASS [TIME] test::test_v2
    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_test_regex_quantifier() {
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

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg(r"test(/test_ab+$/)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_ab
            PASS [TIME] test::test_abb
    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_test_regex_qualified_name_prefix() {
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

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("test(/^test::test_log/)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_login
            PASS [TIME] test::test_logout
    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_test_substring_case_sensitive() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_Alpha():
    assert True

def test_alpha():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("test(~Alpha)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_Alpha
    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_test_regex_case_insensitive() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_Alpha():
    assert True

def test_alpha():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("test(/(?i)alpha/)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_Alpha
            PASS [TIME] test::test_alpha
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_test_regex_dot_metacharacter() {
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

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg(r"test(/test_a\d/)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_a1
            PASS [TIME] test::test_a2
    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_test_exact_matcher() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(
        context.command_no_parallel().arg("-E").arg("test(=test::test_alpha)"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_alpha
    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn filterset_test_glob_matcher() {
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
    assert_cmd_snapshot!(
        context.command_no_parallel().arg("-E").arg("test(#*log*)"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_login
            PASS [TIME] test::test_logout
    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn filterset_tag_include() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
def test_slow():
    assert True

def test_fast():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("tag(slow)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_slow
    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_tag_exclude() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
def test_slow():
    assert True

def test_fast():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("not tag(slow)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_fast
    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_tag_and() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
@karva.tags.integration
def test_slow_integration():
    assert True

@karva.tags.slow
def test_slow_only():
    assert True

@karva.tags.integration
def test_integration_only():
    assert True
        ",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("-E").arg("tag(slow) & tag(integration)"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_slow_integration
    ────────────
         Summary [TIME] 3 tests run: 1 passed, 2 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn filterset_tag_or() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
def test_slow():
    assert True

@karva.tags.integration
def test_integration():
    assert True

def test_untagged():
    assert True
        ",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("-E").arg("tag(slow) | tag(integration)"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_slow
            PASS [TIME] test::test_integration
    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn filterset_tag_multiple_flags_or_semantics() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
@karva.tags.integration
def test_both():
    assert True

@karva.tags.slow
def test_slow_only():
    assert True

@karva.tags.integration
def test_integration_only():
    assert True
        ",
    );

    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("-E")
            .arg("tag(slow)")
            .arg("-E")
            .arg("tag(integration)"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_both
            PASS [TIME] test::test_slow_only
            PASS [TIME] test::test_integration_only
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn filterset_tag_no_matches() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_untagged():
    assert True

@karva.tags.fast
def test_fast():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("tag(slow)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_tag_with_parametrize() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
@karva.tags.parametrize('x', [1, 2])
def test_param(x):
    assert x > 0

def test_untagged():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("tag(slow)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_param(x=1)
            PASS [TIME] test::test_param(x=2)
    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_tag_and_not_via_minus() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
@karva.tags.flaky
def test_slow_flaky():
    assert True

@karva.tags.slow
def test_slow_stable():
    assert True

def test_untagged():
    assert True
        ",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("-E").arg("tag(slow) - tag(flaky)"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_slow_stable
    ────────────
         Summary [TIME] 3 tests run: 1 passed, 2 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn filterset_tag_parenthesized_expression() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
@karva.tags.linux
def test_slow_linux():
    assert True

@karva.tags.fast
@karva.tags.linux
def test_fast_linux():
    assert True

@karva.tags.slow
def test_slow_only():
    assert True

@karva.tags.linux
def test_linux_only():
    assert True
        ",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("-E").arg("(tag(slow) | tag(fast)) & tag(linux)"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 4 tests across 1 worker
            PASS [TIME] test::test_slow_linux
            PASS [TIME] test::test_fast_linux
    ────────────
         Summary [TIME] 4 tests run: 2 passed, 2 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn filterset_tag_pytest_marks() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.mark.slow
def test_slow():
    assert True

@pytest.mark.slow("reason", key="value")
def test_slow_with_args():
    assert True

def test_untagged():
    assert True
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("tag(slow)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_slow
            PASS [TIME] test::test_slow_with_args
    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_tag_with_skip_decorator() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.slow
@karva.tags.skip
def test_slow_skipped():
    assert False

@karva.tags.slow
def test_slow_runs():
    assert True

def test_untagged():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("tag(slow)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_slow_runs
    ────────────
         Summary [TIME] 3 tests run: 1 passed, 2 skipped

    ----- stderr -----
    ");
}

#[test]
fn filterset_tag_pytest_param_marks() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import pytest

@pytest.mark.parametrize("x", [
    pytest.param(1),
    pytest.param(2, marks=pytest.mark.slow),
    pytest.param(3, marks=[pytest.mark.slow, pytest.mark.integration]),
])
def test_with_custom_marks(x):
    assert x > 0
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("tag(slow)"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_with_custom_marks(x=2)
            PASS [TIME] test::test_with_custom_marks(x=3)
    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 skipped

    ----- stderr -----
    ");
}

const MIXED: &str = r"
import karva

@karva.tags.slow
@karva.tags.integration
def test_slow_integration_login():
    assert True

@karva.tags.slow
def test_slow_logout():
    assert True

@karva.tags.integration
def test_integration_signup():
    assert True

def test_plain_login():
    assert True
";

#[test]
fn filterset_test_and_tag_combined() {
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

    assert_cmd_snapshot!(
        context.command_no_parallel().arg("-E").arg("tag(slow) & test(~alpha)"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_slow_alpha
    ────────────
         Summary [TIME] 3 tests run: 1 passed, 2 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn filterset_parenthesized_or_with_and() {
    let context = TestContext::with_file("test.py", MIXED);
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .arg("-E")
            .arg("(tag(slow) | tag(integration)) & test(~login)"),
        @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 4 tests across 1 worker
            PASS [TIME] test::test_slow_integration_login
    ────────────
         Summary [TIME] 4 tests run: 1 passed, 3 skipped

    ----- stderr -----
    "
    );
}

#[test]
fn filterset_invalid_regex() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("test(/[invalid/)"), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: invalid `--filter` expression
      Cause: invalid regex `/[invalid/` in filter expression `test(/[invalid/)`: regex parse error:
        [invalid
        ^
    error: unclosed character class
    ");
}

#[test]
fn filterset_invalid_regex_unclosed_group() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("test(/(unclosed/)"), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: invalid `--filter` expression
      Cause: invalid regex `/(unclosed/` in filter expression `test(/(unclosed/)`: regex parse error:
        (unclosed
        ^
    error: unclosed group
    ");
}

#[test]
fn filterset_invalid_regex_invalid_repetition() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(context.command_no_parallel().arg("-E").arg("test(/*invalid/)"), @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: invalid `--filter` expression
      Cause: invalid regex `/*invalid/` in filter expression `test(/*invalid/)`: regex parse error:
        *invalid
        ^
    error: repetition operator missing expression
    ");
}

#[test]
fn filterset_invalid_regex_bad_escape() {
    let context = TestContext::with_file("test.py", TWO_TESTS);
    assert_cmd_snapshot!(
        context.command_no_parallel().arg("-E").arg(r"test(/\p{Invalid}/)"),
        @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: invalid `--filter` expression
      Cause: invalid regex `/\p{Invalid}/` in filter expression `test(/\p{Invalid}/)`: regex parse error:
        \p{Invalid}
        ^^^^^^^^^^^
    error: Unicode property not found
    "
    );
}

#[test]
fn filterset_unknown_predicate() {
    let context = TestContext::with_file("test.py", "def test_x(): assert True\n");
    assert_cmd_snapshot!(
        context.command_no_parallel().arg("-E").arg("package(foo)"),
        @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: invalid `--filter` expression
      Cause: unknown predicate `package` in filter expression `package(foo)` (expected `test` or `tag`)
    "
    );
}

#[test]
fn filterset_unclosed_paren() {
    let context = TestContext::with_file("test.py", "def test_x(): assert True\n");
    assert_cmd_snapshot!(
        context.command_no_parallel().arg("-E").arg("tag(slow"),
        @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: invalid `--filter` expression
      Cause: expected closing `)` in filter expression `tag(slow`
    "
    );
}

#[test]
fn filterset_empty_matcher_body() {
    let context = TestContext::with_file("test.py", "def test_x(): assert True\n");
    assert_cmd_snapshot!(
        context.command_no_parallel().arg("-E").arg("tag()"),
        @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: invalid `--filter` expression
      Cause: expected a matcher body in filter expression `tag()`
    "
    );
}

#[test]
fn filterset_empty_expression() {
    let context = TestContext::with_file("test.py", "def test_x(): assert True\n");
    assert_cmd_snapshot!(
        context.command_no_parallel().arg("-E").arg(""),
        @r"
    success: false
    exit_code: 2
    ----- stdout -----

    ----- stderr -----
    Karva failed
      Cause: invalid `--filter` expression
      Cause: empty filter expression ``
    "
    );
}
