use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

fn get_expect_fail_decorator(framework: &str) -> &str {
    match framework {
        "pytest" => "pytest.mark.xfail",
        "karva" => "karva.tags.expect_fail",
        _ => panic!("Invalid framework"),
    }
}

#[rstest]
fn test_expect_fail_that_fails(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(reason='Known bug')
def test_1():
    assert False, 'This test is expected to fail'
        ",
            decorator = get_expect_fail_decorator(framework)
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
fn test_expect_fail_that_passes_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected to fail but passes')
def test_1():
    assert True
        ",
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
                FAIL [TIME] test::test_1

        failures:

        test::test_1:

        error[test-pass-on-expect-failure]: Test `test_1` passes when expected to fail
         --> test.py:5:5
          |
        5 | def test_1():
          |     ^^^^^^
        info: Reason: Expected to fail but passes

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_expect_fail_that_passes_pytest() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

@pytest.mark.xfail(reason='Expected to fail but passes')
def test_1():
    assert True
        ",
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
                FAIL [TIME] test::test_1

        failures:

        test::test_1:

        error[test-pass-on-expect-failure]: Test `test_1` passes when expected to fail
         --> test.py:5:5
          |
        5 | def test_1():
          |     ^^^^^^
        info: Reason: Expected to fail but passes

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_expect_fail_no_reason(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}
def test_1():
    assert False
        ",
            decorator = get_expect_fail_decorator(framework)
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
fn test_expect_fail_with_call(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}()
def test_1():
    assert False
        ",
            decorator = get_expect_fail_decorator(framework)
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
fn test_expect_fail_with_true_condition(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(True, reason='Condition is true')
def test_1():
    assert False
        ",
            decorator = get_expect_fail_decorator(framework)
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
fn test_expect_fail_with_false_condition(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(False, reason='Condition is false')
def test_1():
    assert True
        ",
            decorator = get_expect_fail_decorator(framework)
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
fn test_pytest_expect_fail_with_false_keyword_condition() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

@pytest.mark.xfail(condition=False, reason='Condition is false')
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

#[rstest]
fn test_expect_fail_with_expression(#[values("pytest", "karva")] framework: &str) {
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
            decorator = get_expect_fail_decorator(framework)
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
fn test_expect_fail_with_multiple_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(True, False, reason='Multiple conditions with one true')
def test_1():
    assert False
        ",
            decorator = get_expect_fail_decorator(framework)
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
fn test_expect_fail_with_all_false_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(False, False, reason='All conditions false')
def test_1():
    assert True
        ",
            decorator = get_expect_fail_decorator(framework)
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
fn test_expect_fail_with_single_string_as_reason_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail('This is expected to fail')
def test_1():
    assert False
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

#[test]
fn test_expect_fail_with_empty_conditions_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail()
def test_1():
    assert False
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

#[rstest]
fn test_expect_fail_mixed_tests_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected to fail')
def test_expected_to_fail():
    assert False

def test_normal_pass():
    assert True

@karva.tags.expect_fail()
def test_expected_fail_passes():
    assert True
        ",
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 3 tests across 1 worker
                PASS [TIME] test::test_expected_to_fail
                PASS [TIME] test::test_normal_pass
                FAIL [TIME] test::test_expected_fail_passes

        failures:

        test::test_expected_fail_passes:

        error[test-pass-on-expect-failure]: Test `test_expected_fail_passes` passes when expected to fail
          --> test.py:12:5
           |
        12 | def test_expected_fail_passes():
           |     ^^^^^^^^^^^^^^^^^^^^^^^^^

        ────────────
             Summary [TIME] 3 tests run: 2 passed, 1 failed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn test_expect_fail_mixed_tests_pytest() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

@pytest.mark.xfail(reason='Expected to fail')
def test_expected_to_fail():
    assert False

def test_normal_pass():
    assert True

@pytest.mark.xfail
def test_expected_fail_passes():
    assert True
        ",
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 3 tests across 1 worker
                PASS [TIME] test::test_expected_to_fail
                PASS [TIME] test::test_normal_pass
                FAIL [TIME] test::test_expected_fail_passes

        failures:

        test::test_expected_fail_passes:

        error[test-pass-on-expect-failure]: Test `test_expected_fail_passes` passes when expected to fail
          --> test.py:12:5
           |
        12 | def test_expected_fail_passes():
           |     ^^^^^^^^^^^^^^^^^^^^^^^^^

        ────────────
             Summary [TIME] 3 tests run: 2 passed, 1 failed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn test_expect_fail_with_runtime_error() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected to fail with runtime error')
def test_1():
    raise RuntimeError('Something went wrong')
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

#[test]
fn test_expect_fail_does_not_retry_expected_failure() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected to fail')
def test_1():
    assert False
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=2"), @"
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

#[test]
fn test_expect_fail_with_returned_value() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected returned value')
def test_1():
    return False
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=2"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_1
      TRY 2 FAIL [TIME] test::test_1
      TRY 3 FAIL [TIME] test::test_1

    failures:

    test::test_1:

    error[test-returned-value]: Test `test_1` returned `False`
     --> test.py:5:5
      |
    5 | def test_1():
      |     ^^^^^^
    info: Test functions must return None. Did you mean to use `assert`?

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_expect_fail_with_assertion_error() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected to fail')
def test_1():
    raise AssertionError('This assertion should fail')
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

#[test]
fn test_expect_fail_with_skip() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected to fail')
def test_1():
    karva.skip('Skipping this test')
    assert False
        ",
    );

    // Skip takes precedence - test should be skipped, not treated as expected fail
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
fn test_expect_fail_then_unexpected_pass() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail(reason='This should fail but passes')
def test_should_fail():
    assert 1 + 1 == 2
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_should_fail

    failures:

    test::test_should_fail:

    error[test-pass-on-expect-failure]: Test `test_should_fail` passes when expected to fail
     --> test.py:5:5
      |
    5 | def test_should_fail():
      |     ^^^^^^^^^^^^^^^^
    info: Reason: This should fail but passes

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn test_expect_fail_with_parametrize(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{parametrize}('x', [1, 2, 3])
@{expect_fail}
def test_param(x):
    assert x > 10
        ",
            expect_fail = get_expect_fail_decorator(framework),
            parametrize = if framework == "pytest" {
                "pytest.mark.parametrize"
            } else {
                "karva.tags.parametrize"
            }
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_param(x=1)
                PASS [TIME] test::test_param(x=2)
                PASS [TIME] test::test_param(x=3)
        ────────────
             Summary [TIME] 3 tests run: 3 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn test_pytest_expect_fail_empty_list_condition_does_not_expect_fail() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

@pytest.mark.xfail([], reason='empty list is false')
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

#[test]
fn test_pytest_expect_fail_boolean_condition_without_reason_rejected() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

@pytest.mark.xfail(False)
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

    error[failed-to-import-module]: Failed to import python module `test`: pytest xfail mark requires a reason when using boolean conditions

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_pytest_expect_fail_keyword_condition_without_reason_rejected() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

@pytest.mark.xfail(condition=True)
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

    error[failed-to-import-module]: Failed to import python module `test`: pytest xfail mark requires a reason when using boolean conditions

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_pytest_expect_fail_non_string_reason_is_preserved() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

@pytest.mark.xfail(reason=123)
def test_1():
    assert True
",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_1

    failures:

    test::test_1:

    error[test-pass-on-expect-failure]: Test `test_1` passes when expected to fail
     --> test.py:5:5
      |
    5 | def test_1():
      |     ^^^^^^
    info: Reason: 123

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_pytest_expect_fail_string_condition_uses_module_globals() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

SHOULD_EXPECT_FAIL = False

@pytest.mark.xfail('SHOULD_EXPECT_FAIL', reason='module global')
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

#[test]
fn test_pytest_expect_fail_string_condition_without_reason_reports_condition() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

SHOULD_EXPECT_FAIL = True

@pytest.mark.xfail('SHOULD_EXPECT_FAIL')
def test_1():
    assert True
",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_1

    failures:

    test::test_1:

    error[test-pass-on-expect-failure]: Test `test_1` passes when expected to fail
     --> test.py:7:5
      |
    7 | def test_1():
      |     ^^^^^^
    info: Reason: condition: SHOULD_EXPECT_FAIL

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn expected_exception_matches_subclasses(#[values("karva", "pytest")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

class SpecificError(ValueError):
    pass

@{decorator}(raises=ValueError, reason='specific bug')
def test_expected():
    raise SpecificError('known')
",
            decorator = get_expect_fail_decorator(framework),
        ),
    );
    allow_duplicates! {
        assert_cmd_snapshot!(context.command().arg("--retry=1"), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_expected
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn expected_exception_matches_any_class_in_tuple(#[values("karva", "pytest")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(raises=(KeyError, ValueError), reason='specific bug')
def test_expected():
    raise ValueError('known')
",
            decorator = get_expect_fail_decorator(framework),
        ),
    );
    allow_duplicates! {
        assert_cmd_snapshot!(context.command().arg("--retry=1"), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_expected
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn expected_failure_rejects_mismatching_exception_and_retries(
    #[values("karva", "pytest")] framework: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(raises=ValueError, reason='body only')
def test_expected():
    raise TypeError('unexpected')
",
            decorator = get_expect_fail_decorator(framework),
        ),
    );
    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
          TRY 1 FAIL [TIME] test::test_expected
          TRY 2 FAIL [TIME] test::test_expected

        failures:

        test::test_expected:

        error[test-failure]: Test `test_expected` failed
         --> test.py:5:5
          |
        5 | def test_expected():
          |     ^^^^^^^^^^^^^
        info: Test failed here
         --> test.py:6:5
          |
        6 |     raise TypeError('unexpected')
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: unexpected

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn expected_failure_does_not_absorb_fixture_setup_failure(
    #[values("karva", "pytest")] framework: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}
from karva import fixture

@fixture
def value():
    raise ValueError('setup failed')

@{decorator}(raises=Exception, reason='body only')
def test_expected(value):
    raise ValueError('expected body')
",
            decorator = get_expect_fail_decorator(framework),
        ),
    );
    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
         TRY 1 ERROR [TIME] test::test_expected
         TRY 2 ERROR [TIME] test::test_expected

        failures:

        test::test_expected (requires fixture `value`):

        error[fixture-failure]: Fixture `value` failed
         --> test.py:6:5
          |
        6 | def value():
          |     ^^^^^
        info: Fixture failed here
         --> test.py:7:5
          |
        7 |     raise ValueError('setup failed')
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: setup failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn expected_failure_does_not_absorb_fixture_teardown_failure(
    #[values("karva", "pytest")] framework: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}
from karva import fixture

@fixture
def value():
    yield 1
    raise ValueError('teardown failed')

@{decorator}(raises=Exception, reason='body only')
def test_expected(value):
    raise ValueError('expected body')
",
            decorator = get_expect_fail_decorator(framework),
        ),
    );
    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
         TRY 1 ERROR [TIME] test::test_expected(value=1)
         TRY 2 ERROR [TIME] test::test_expected(value=1)

        failures:

        test::test_expected(value=1):

        error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `value`
         --> test.py:6:5
          |
        6 | def value():
          |     ^^^^^
        info: Failed to reset fixture: teardown failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn expected_failure_does_not_absorb_missing_fixture(#[values("karva", "pytest")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(raises=Exception, reason='body only')
def test_expected(value):
    raise ValueError('expected body')
",
            decorator = get_expect_fail_decorator(framework),
        ),
    );
    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test::test_expected

        failures:

        test::test_expected:

        error[missing-fixtures]: Test `test_expected` has missing fixtures
         --> test.py:5:5
          |
        5 | def test_expected(value):
          |     ^^^^^^^^^^^^^
        info: Missing fixtures: `value`

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn expected_failure_does_not_absorb_non_none_return(#[values("karva", "pytest")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(raises=Exception, reason='body only')
def test_expected():
    return 1
",
            decorator = get_expect_fail_decorator(framework),
        ),
    );
    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
          TRY 1 FAIL [TIME] test::test_expected
          TRY 2 FAIL [TIME] test::test_expected

        failures:

        test::test_expected:

        error[test-returned-value]: Test `test_expected` returned `1`
         --> test.py:5:5
          |
        5 | def test_expected():
          |     ^^^^^^^^^^^^^
        info: Test functions must return None. Did you mean to use `assert`?

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn expected_failure_does_not_absorb_async_timeout(#[values("karva", "pytest")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import asyncio
import {framework}
from karva import tags

@tags.timeout(0.1)
@{decorator}(raises=Exception, reason='body only')
async def test_expected():
    await asyncio.sleep(2)
",
            decorator = get_expect_fail_decorator(framework),
        ),
    );
    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
          TRY 1 FAIL [TIME] test::test_expected
          TRY 2 FAIL [TIME] test::test_expected

        failures:

        test::test_expected:

        error[test-failure]: Test `test_expected` failed
         --> test.py:8:11
          |
        8 | async def test_expected():
          |           ^^^^^^^^^^^^^
        info: Test exceeded timeout of 0.1 seconds

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn expected_failure_does_not_absorb_background_task_exception(
    #[values("karva", "pytest")] framework: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import asyncio
import {framework}

async def broken():
    raise ValueError('background failed')

@{decorator}(raises=Exception, reason='body only')
async def test_expected():
    asyncio.create_task(broken())
    await asyncio.sleep(0)
",
            decorator = get_expect_fail_decorator(framework),
        ),
    );
    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
          TRY 1 FAIL [TIME] test::test_expected
          TRY 2 FAIL [TIME] test::test_expected

        failures:

        test::test_expected:

        error[test-failure]: Test `test_expected` failed
         --> test.py:9:11
          |
        9 | async def test_expected():
          |           ^^^^^^^^^^^^^
        info: Test failed here
         --> test.py:6:5
          |
        6 |     raise ValueError('background failed')
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: Unhandled exception in background task: Task-[N]: ValueError: background failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn expected_exception_policy_rejects_non_class(#[values("karva", "pytest")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(raises=42)
def test_expected():
    pass
",
            decorator = get_expect_fail_decorator(framework),
        ),
    );
    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
        diagnostics:

        error[failed-to-import-module]: Failed to import python module `test`: expect_fail raises must be an exception class or a tuple of exception classes

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn expected_exception_policy_rejects_non_exception_class(
    #[values("karva", "pytest")] framework: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(raises=str)
def test_expected():
    pass
",
            decorator = get_expect_fail_decorator(framework),
        ),
    );
    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
        diagnostics:

        error[failed-to-import-module]: Failed to import python module `test`: expect_fail raises must be an exception class or a tuple of exception classes

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn expected_exception_policy_rejects_tuple_with_non_class(
    #[values("karva", "pytest")] framework: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(raises=(ValueError, 42))
def test_expected():
    pass
",
            decorator = get_expect_fail_decorator(framework),
        ),
    );
    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
        diagnostics:

        error[failed-to-import-module]: Failed to import python module `test`: expect_fail raises must be an exception class or a tuple of exception classes

        ────────────
             Summary [TIME] 0 tests run: 0 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn expected_failure_matches_sync_body_timeout_error(#[values("karva", "pytest")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}
from karva import tags

@tags.timeout(1)
@{decorator}(raises=TimeoutError, reason='body exception')
def test_expected():
    raise TimeoutError('test body failed')
",
            decorator = get_expect_fail_decorator(framework),
        ),
    );
    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_expected
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn expected_failure_matches_async_body_timeout_error(#[values("karva", "pytest")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}
from karva import tags

@tags.timeout(1)
@{decorator}(raises=TimeoutError, reason='body exception')
async def test_expected():
    raise TimeoutError('test body failed')
",
            decorator = get_expect_fail_decorator(framework),
        ),
    );
    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 1 test across 1 worker
                PASS [TIME] test::test_expected
        ────────────
             Summary [TIME] 1 test run: 1 passed, 0 skipped

        ----- stderr -----
        ");
    }
}
