use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

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

    diagnostics:

    error[test-failure]: Test `test_with_fail_with_reason` failed
     --> test.py:4:5
      |
    4 | def test_with_fail_with_reason():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:5:5
      |
    5 |     karva.fail('This is a custom failure message')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: This is a custom failure message

    error[test-failure]: Test `test_with_fail_with_no_reason` failed
     --> test.py:7:5
      |
    7 | def test_with_fail_with_no_reason():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:8:5
      |
    8 |     karva.fail()
      |     ^^^^^^^^^^^^
      |

    error[test-failure]: Test `test_with_fail_with_keyword_reason` failed
      --> test.py:10:5
       |
    10 | def test_with_fail_with_keyword_reason():
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
       |
    info: Test failed here
      --> test.py:11:5
       |
    11 |     karva.fail(reason='This is a custom failure message')
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
       |
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

    diagnostics:

    error[test-failure]: Test `test_conditional_fail` failed
     --> test.py:4:5
      |
    4 | def test_conditional_fail():
      |     ^^^^^^^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:7:9
      |
    7 |         karva.fail('failing test')
      |         ^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
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

    diagnostics:

    error[test-failure]: Test `test_raise_fail_error` failed
     --> test.py:4:5
      |
    4 | def test_raise_fail_error():
      |     ^^^^^^^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:5:5
      |
    5 |     raise karva.FailError('Manually raised FailError')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
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

    diagnostics:

    error[test-failure]: Test `test_raises_no_exception` failed
     --> test.py:4:5
      |
    4 | def test_raises_no_exception():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:5:5
      |
    5 |     with karva.raises(ValueError):
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
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

    diagnostics:

    error[test-failure]: Test `test_raises_match_fails` failed
     --> test.py:4:5
      |
    4 | def test_raises_match_fails():
      |     ^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:5:5
      |
    5 |     with karva.raises(ValueError, match='xyz'):
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
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

    diagnostics:

    error[test-failure]: Test `test_raises_wrong_type` failed
     --> test.py:4:5
      |
    4 | def test_raises_wrong_type():
      |     ^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Test failed here
     --> test.py:6:9
      |
    6 |         raise TypeError('wrong type')
      |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
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
