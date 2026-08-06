use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[test]
fn test_approx_matches_pytest() {
    let context = TestContext::with_file(
        "test.py",
        r#"
from decimal import Decimal
import math

import karva
import pytest

@karva.tags.parametrize("actual, expected, kwargs, result", [
    (0.1 + 0.2, 0.3, {}, True),
    (1.0001, 1, {}, False),
    (1.0001, 1, {"rel": 1e-3}, True),
    (1 + 1e-8, 1, {"abs": 1e-12}, False),
    (1e-13, 0, {}, True),
    (complex(1.0000001, 2), complex(1, 2), {}, True),
    (Decimal("1.0000001"), Decimal("1"), {}, True),
    ([0.1 + 0.2, 0.6], [0.3, 0.6], {}, True),
    ([0.3], [0.3, 0.6], {}, False),
    ({"x": 0.1 + 0.2}, {"x": 0.3}, {}, True),
    ({"y": 0.3}, {"x": 0.3}, {}, False),
    (math.nan, math.nan, {}, False),
    (math.nan, math.nan, {"nan_ok": True}, True),
    (math.inf, math.inf, {}, True),
    (-math.inf, math.inf, {}, False),
])
def test_supported_cases_match_pytest(actual, expected, kwargs, result):
    assert (actual == karva.approx(expected, **kwargs)) == result
    assert (actual == karva.approx(expected, **kwargs)) == (actual == pytest.approx(expected, **kwargs))

def test_works_on_either_side():
    assert karva.approx(0.3) == 0.1 + 0.2
    assert 0.1 + 0.2 == karva.approx(0.3)

def test_representation_includes_tolerance():
    assert "0.3 ± 3.0e-07" == repr(karva.approx(0.3))
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_supported_cases_match_pytest(actual=0.30000000000000004, expected=0.3, kwargs={}, result=True)
            PASS [TIME] test::test_supported_cases_match_pytest(actual=1.0001, expected=1, kwargs={}, result=False)
            PASS [TIME] test::test_supported_cases_match_pytest(actual=1.0001, expected=1, kwargs={'rel': 0.001}, result=True)
            PASS [TIME] test::test_supported_cases_match_pytest(actual=1.00000001, expected=1, kwargs={'abs': 1e-12}, result=False)
            PASS [TIME] test::test_supported_cases_match_pytest(actual=1e-13, expected=0, kwargs={}, result=True)
            PASS [TIME] test::test_supported_cases_match_pytest(actual=(1.0000001+2j), expected=(1+2j), kwargs={}, result=True)
            PASS [TIME] test::test_supported_cases_match_pytest(actual=1.0000001, expected=1, kwargs={}, result=True)
            PASS [TIME] test::test_supported_cases_match_pytest(actual=[0.30000000000000004, 0.6], expected=[0.3, 0.6], kwargs={}, result=True)
            PASS [TIME] test::test_supported_cases_match_pytest(actual=[0.3], expected=[0.3, 0.6], kwargs={}, result=False)
            PASS [TIME] test::test_supported_cases_match_pytest(actual={'x': 0.30000000000000004}, expected={'x': 0.3}, kwargs={}, result=True)
            PASS [TIME] test::test_supported_cases_match_pytest(actual={'y': 0.3}, expected={'x': 0.3}, kwargs={}, result=False)
            PASS [TIME] test::test_supported_cases_match_pytest(actual=nan, expected=nan, kwargs={}, result=False)
            PASS [TIME] test::test_supported_cases_match_pytest(actual=nan, expected=nan, kwargs={'nan_ok': True}, result=True)
            PASS [TIME] test::test_supported_cases_match_pytest(actual=inf, expected=inf, kwargs={}, result=True)
            PASS [TIME] test::test_supported_cases_match_pytest(actual=-inf, expected=inf, kwargs={}, result=False)
            PASS [TIME] test::test_works_on_either_side
            PASS [TIME] test::test_representation_includes_tolerance
    ────────────
         Summary [TIME] 17 tests run: 17 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_approx_invalid_value_diagnostic() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_rejects_invalid_sequence_value():
    karva.approx([1, "two"])

def test_rejects_invalid_mapping_value():
    karva.approx({"count": "two"})

def test_rejects_invalid_scalar_value():
    karva.approx("two")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 3 tests across 1 worker
            FAIL [TIME] test::test_rejects_invalid_sequence_value
            FAIL [TIME] test::test_rejects_invalid_mapping_value
            FAIL [TIME] test::test_rejects_invalid_scalar_value

    failures:

    test::test_rejects_invalid_mapping_value:

    error[test-failure]: Test `test_rejects_invalid_mapping_value` failed
     --> test.py:7:5
    7 | def test_rejects_invalid_mapping_value():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test.py:8:5
    8 |     karva.approx({"count": "two"})
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: karva.approx() expected a numeric value at key 'count', got str: 'two'

    test::test_rejects_invalid_scalar_value:

    error[test-failure]: Test `test_rejects_invalid_scalar_value` failed
      --> test.py:10:5
    10 | def test_rejects_invalid_scalar_value():
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
      --> test.py:11:5
    11 |     karva.approx("two")
       |     ^^^^^^^^^^^^^^^^^^^
    info: karva.approx() expected a numeric value, got str: 'two'

    test::test_rejects_invalid_sequence_value:

    error[test-failure]: Test `test_rejects_invalid_sequence_value` failed
     --> test.py:4:5
    4 | def test_rejects_invalid_sequence_value():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test.py:5:5
    5 |     karva.approx([1, "two"])
      |     ^^^^^^^^^^^^^^^^^^^^^^^^
    info: karva.approx() expected a numeric value at index 1, got str: 'two'

    ────────────
         Summary [TIME] 3 tests run: 0 passed, 3 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn test_fail_function() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_with_fail_with_reason():
    karva.fail('This is a custom failure message')

def test_with_fail_with_no_reason():
    karva.fail()

def test_with_fail_with_keyword_reason():
    karva.fail(reason='This is a custom failure message')

        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 3 tests across 1 worker
            FAIL [TIME] test::test_with_fail_with_reason
            FAIL [TIME] test::test_with_fail_with_no_reason
            FAIL [TIME] test::test_with_fail_with_keyword_reason

    failures:

    test::test_with_fail_with_keyword_reason:

    error[test-failure]: Test `test_with_fail_with_keyword_reason` failed
      --> test.py:10:5
    10 | def test_with_fail_with_keyword_reason():
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
      --> test.py:11:5
    11 |     karva.fail(reason='This is a custom failure message')
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: This is a custom failure message

    test::test_with_fail_with_no_reason:

    error[test-failure]: Test `test_with_fail_with_no_reason` failed
     --> test.py:7:5
    7 | def test_with_fail_with_no_reason():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test.py:8:5
    8 |     karva.fail()
      |     ^^^^^^^^^^^^

    test::test_with_fail_with_reason:

    error[test-failure]: Test `test_with_fail_with_reason` failed
     --> test.py:4:5
    4 | def test_with_fail_with_reason():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test.py:5:5
    5 |     karva.fail('This is a custom failure message')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: This is a custom failure message

    ────────────
         Summary [TIME] 3 tests run: 0 passed, 3 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fail_function_conditional() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_conditional_fail():
    condition = True
    if condition:
        karva.fail('failing test')
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_conditional_fail

    failures:

    test::test_conditional_fail:

    error[test-failure]: Test `test_conditional_fail` failed
     --> test.py:4:5
    4 | def test_conditional_fail():
      |     ^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test.py:7:9
    7 |         karva.fail('failing test')
      |         ^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: failing test

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_fail_error_exception() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_raise_fail_error():
    raise karva.FailError('Manually raised FailError')
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_raise_fail_error

    failures:

    test::test_raise_fail_error:

    error[test-failure]: Test `test_raise_fail_error` failed
     --> test.py:4:5
    4 | def test_raise_fail_error():
      |     ^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test.py:5:5
    5 |     raise karva.FailError('Manually raised FailError')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Manually raised FailError

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn test_runtime_skip_pytest(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

def test_skip_with_reason():
    {framework}.skip('This test is skipped at runtime')
    assert False, 'This should not be reached'

def test_skip_without_reason():
    {framework}.skip()
    assert False, 'This should not be reached'

def test_conditional_skip():
    condition = True
    if condition:
        {framework}.skip('Condition was true')
    assert False, 'This should not be reached'
        "
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: true
        exit_code: 0
        ----- stdout -----
            Starting 3 tests across 1 worker
        ────────────
             Summary [TIME] 3 tests run: 0 passed, 3 skipped

        ----- stderr -----
        ");
    }
}

#[test]
fn test_module_level_skip_with_passing_module() {
    let context = TestContext::with_files([
        (
            "test_skipped.py",
            r#"
import karva

karva.skip("no gpu available")

def test_never_reached():
    assert False
            "#,
        ),
        (
            "test_passed.py",
            r"
def test_passes():
    assert True
            ",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=skip"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            SKIP [TIME] test_skipped::<module>: no gpu available
            PASS [TIME] test_passed::test_passes
    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_all_modules_skipped_at_module_level() {
    let context = TestContext::with_file(
        "test_gpu.py",
        r#"
import karva

karva.skip("no gpu available")

def test_gpu():
    assert False

def test_more_gpu():
    assert False
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=skip"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            SKIP [TIME] test_gpu::<module>: no gpu available
    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn test_runtime_skip_does_not_retry(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

def test_skip():
    {framework}.skip('This test is skipped at runtime')
    assert False, 'This should not be reached'
        "
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=2"), @"
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
fn test_runtime_skip_after_failed_attempt() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import karva

def test_skip_on_retry():
    if os.environ["KARVA_ATTEMPT"] == "1":
        assert False
    karva.skip("skip after retry")
        "#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=2"), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
      TRY 1 FAIL [TIME] test::test_skip_on_retry
      TRY 2 SKIP [TIME] test::test_skip_on_retry
    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_mixed_skip_and_pass() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_pass():
    assert True

def test_skip():
    karva.skip('Skipped test')
    assert False

def test_another_pass():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_pass
            PASS [TIME] test::test_another_pass
    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_skip_error_exception() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_raise_skip_error():
    raise karva.SkipError('Manually raised SkipError')
    assert False, 'This should not be reached'
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
fn test_raises_matching_exception() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_raises_value_error():
    with karva.raises(ValueError):
        raise ValueError('oops')
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_raises_value_error
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_raises_no_exception() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_raises_no_exception():
    with karva.raises(ValueError):
        pass
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_raises_no_exception

    failures:

    test::test_raises_no_exception:

    error[test-failure]: Test `test_raises_no_exception` failed
     --> test.py:4:5
    4 | def test_raises_no_exception():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test.py:5:5
    5 |     with karva.raises(ValueError):
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: DID NOT RAISE <class 'ValueError'>

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_raises_with_match() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_raises_match_passes():
    with karva.raises(ValueError, match='oops'):
        raise ValueError('oops something happened')
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_raises_match_passes
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_raises_with_match_fails() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_raises_match_fails():
    with karva.raises(ValueError, match='xyz'):
        raise ValueError('oops')
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_raises_match_fails

    failures:

    test::test_raises_match_fails:

    error[test-failure]: Test `test_raises_match_fails` failed
     --> test.py:4:5
    4 | def test_raises_match_fails():
      |     ^^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test.py:5:5
    5 |     with karva.raises(ValueError, match='xyz'):
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Raised exception did not match pattern 'xyz'

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_raises_wrong_exception_type() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_raises_wrong_type():
    with karva.raises(ValueError):
        raise TypeError('wrong type')
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_raises_wrong_type

    failures:

    test::test_raises_wrong_type:

    error[test-failure]: Test `test_raises_wrong_type` failed
     --> test.py:4:5
    4 | def test_raises_wrong_type():
      |     ^^^^^^^^^^^^^^^^^^^^^^
    info: Test failed here
     --> test.py:6:9
    6 |         raise TypeError('wrong type')
      |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: wrong type

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_raises_exc_info() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

def test_raises_exc_info():
    with karva.raises(ValueError) as exc_info:
        raise ValueError('info test')
    assert str(exc_info.value) == 'info test'
    assert exc_info.type is ValueError
    assert exc_info.tb is not None
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_raises_exc_info
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_raises_subclass() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

class CustomError(ValueError):
    pass

def test_raises_subclass():
    with karva.raises(ValueError):
        raise CustomError('subclass')
        ",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_raises_subclass
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}
