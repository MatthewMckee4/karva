use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

fn get_skip_function(framework: &str) -> &str {
    match framework {
        "pytest" => "pytest.mark.skip",
        "karva" => "karva.tags.skip",
        _ => panic!("Invalid framework"),
    }
}

fn get_skip_decorator(framework: &str) -> &str {
    match framework {
        "pytest" => "pytest.mark.skipif",
        "karva" => "karva.tags.skip",
        _ => panic!("Invalid framework"),
    }
}

#[rstest]
fn test_skip(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}('This test is skipped with decorator')
def test_1():
    assert False

        ",
            decorator = get_skip_function(framework)
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_skip_keyword(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(reason='This test is skipped with decorator')
def test_1():
    assert False
        ",
            decorator = get_skip_function(framework)
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_skip_functionality_no_reason(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}
def test_1():
    assert False
        ",
            decorator = get_skip_function(framework)
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_skip_reason_function_call(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}()
def test_1():
    assert False
        ",
            decorator = get_skip_function(framework)
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_skip_with_true_condition(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(True, reason='Condition is true')
def test_1():
    assert False

        ",
            decorator = get_skip_decorator(framework)
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_skip_with_false_condition(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(False, reason='Condition is false')
def test_1():
    assert True
        ",
            decorator = get_skip_decorator(framework)
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_1
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_skip_with_expression(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}
import sys

@{decorator}(sys.version_info >= (3, 0), reason='Python 3 or higher')
def test_1():
    assert False
        ",
            decorator = get_skip_decorator(framework)
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_skip_with_multiple_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(True, False, reason='Multiple conditions with one true')
def test_1():
    assert False
        ",
            decorator = get_skip_decorator(framework)
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn test_skip_with_condition_without_reason_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.skip(True)
def test_1():
    assert False
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn test_pytest_skipif_boolean_condition_without_reason_rejected(
    #[values("False", "condition=False", "condition=True")] condition: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import pytest

@pytest.mark.skipif({condition})
def test_1():
    assert True
"
        ),
    );

    allow_duplicates! {
    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test`: pytest skipif mark requires a reason when using boolean conditions

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
    }
}

#[rstest]
fn test_skip_with_multiple_tests(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(True, reason='Should skip')
def test_skip_this():
    assert False

@{decorator}(False, reason='Should not skip')
def test_run_this():
    assert True

def test_normal():
    assert True
        ",
            decorator = get_skip_decorator(framework)
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 3 tests across 1 worker
                PASS [TIME] test::test_run_this
                PASS [TIME] test::test_normal
        ────────────
             Summary [TIME] 3 tests run: 2 passed, 1 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_skip_with_all_false_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(False, False, reason='All conditions false')
def test_1():
    assert True
        ",
            decorator = get_skip_decorator(framework)
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_1
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn test_skip_with_empty_conditions_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.skip()
def test_1():
    assert False
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_skip_with_single_string_as_reason_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.skip('This is the skip reason')
def test_1():
    assert False
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_skip_with_invalid_condition_integer_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.skip(1, 0, reason='Invalid integer conditions')
def test_1():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test`: Expected boolean values for conditions

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_skip_with_mixed_valid_invalid_conditions_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.skip(True, 'false', reason='Mixed valid and invalid')
def test_1():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test`: Expected boolean values for conditions

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_skipif_true_and_false_conditions_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.skip(True)
@karva.tags.skip(False)
def test_skip_with_true():
    assert False

        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_pytest_skip_non_string_positional_reason_rejected() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

@pytest.mark.skip(123)
def test_1():
    assert False
",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test`: pytest skip mark reason must be a string

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_pytest_skip_non_string_keyword_reason_rejected() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

@pytest.mark.skip(reason=123)
def test_1():
    assert False
",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test`: pytest skip mark reason must be a string

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn test_pytest_skipif_non_string_keyword_reason_rejected(
    #[values("True", "condition=True")] condition: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import pytest

@pytest.mark.skipif({condition}, reason=123)
def test_1():
    assert False
"
        ),
    );

    allow_duplicates! {
    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test`: pytest skipif mark reason must be a string

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
    }
}

#[rstest]
fn test_pytest_skipif_false_condition_allows_non_string_reason(
    #[values("False", "condition=False")] condition: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import pytest

@pytest.mark.skipif({condition}, reason=123)
def test_1():
    assert True
"
        ),
    );

    allow_duplicates! {
    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_1
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
    }
}

#[rstest]
fn test_pytest_skipif_empty_list_condition_does_not_skip(
    #[values("[]", "condition=[]")] condition: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import pytest

@pytest.mark.skipif({condition}, reason='empty list is false')
def test_1():
    assert True
"
        ),
    );

    allow_duplicates! {
    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_1
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
    }
}

#[rstest]
fn test_pytest_skipif_string_condition_uses_module_globals(
    #[values("'SHOULD_SKIP'", "condition='SHOULD_SKIP'")] condition: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import pytest

SHOULD_SKIP = False

@pytest.mark.skipif({condition}, reason='module global')
def test_1():
    assert True
"
        ),
    );

    allow_duplicates! {
    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_1
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
    }
}

#[rstest]
fn test_pytest_skipif_true_keyword_condition(
    #[values("condition=True", "False, condition=True", "condition='True'")] condition: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import pytest

@pytest.mark.skipif({condition}, reason='Condition is true')
def test_1():
    assert False
"
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command().arg("--status-level=all"), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                SKIP [TIME] test::test_1: Condition is true
        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn unittest_skip_test_preserves_cleanup_and_other_failures() {
    let context = TestContext::with_file(
        "test_stdlib_skip.py",
        r#"
import unittest
import karva

class CustomSkip(unittest.SkipTest):
    pass

@karva.fixture
def resource():
    yield
    print("resource cleaned up")

@karva.fixture
def skipped_fixture():
    raise unittest.SkipTest("fixture unavailable")

def test_skip_body(resource):
    raise unittest.SkipTest("example unavailable")

def test_skip_subclass():
    raise CustomSkip("custom skip")

def test_skip_fixture(skipped_fixture):
    assert False, "must not be reached"

def test_ordinary_failure():
    raise ValueError("ordinary failure")
"#,
    );
    assert_cmd_snapshot!(
        context
            .command_no_parallel()
            .args(["--status-level=all", "--show-output=true"]),
        @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 4 tests across 1 worker
    resource cleaned up
            SKIP [TIME] test_stdlib_skip::test_skip_body(resource=None): example unavailable
            SKIP [TIME] test_stdlib_skip::test_skip_subclass: custom skip
            SKIP [TIME] test_stdlib_skip::test_skip_fixture: fixture unavailable
            FAIL [TIME] test_stdlib_skip::test_ordinary_failure

    failures:

    test_stdlib_skip::test_ordinary_failure:

    error[test-failure]: Test `test_ordinary_failure` failed
      --> test_stdlib_skip.py:26:5
       |
    26 | def test_ordinary_failure():
       |     ^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
      --> test_stdlib_skip.py:27:5
       |
    27 |     raise ValueError("ordinary failure")
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: ordinary failure

    ────────────
         Summary [TIME] 4 tests run: 0 passed, 1 failed, 3 skipped

    ----- stderr -----
    "#
    );
}

#[test]
fn unittest_skip_test_during_module_import() {
    let context = TestContext::with_file(
        "test_stdlib_skip.py",
        r#"
import unittest
raise unittest.SkipTest("optional example dependency unavailable")

def test_example():
    assert False, "must not be reached"
"#,
    );
    assert_cmd_snapshot!(context.command().arg("--status-level=all"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            SKIP [TIME] test_stdlib_skip::<module>: optional example dependency unavailable
    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 skipped

    ----- stderr -----
    ");
}
