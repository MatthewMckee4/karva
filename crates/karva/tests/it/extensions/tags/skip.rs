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

#[rstest]
fn test_skip_with_condition_without_reason(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(True)
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

#[rstest]
fn test_skipif_true_and_false_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(True)
@{decorator}(False)
def test_skip_with_true():
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

#[test]
fn test_pytest_skipif_non_string_keyword_reason_rejected() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

@pytest.mark.skipif(True, reason=123)
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

    error[failed-to-import-module]: Failed to import python module `test`: pytest skipif mark reason must be a string

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_pytest_skipif_empty_list_condition_does_not_skip() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

@pytest.mark.skipif([], reason='empty list is false')
def test_1():
    assert True
",
    );

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
