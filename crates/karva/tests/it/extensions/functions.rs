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

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_with_fail_with_reason ... FAILED
    test test::test_with_fail_with_no_reason ... FAILED
    test test::test_with_fail_with_keyword_reason ... FAILED

    diagnostics:

    error[test-failure]: Test `test_with_fail_with_reason` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_with_fail_with_reason():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^
    5 |     karva.fail('This is a custom failure message')
      |
    info: Test failed here
     --> test.py:5:5
      |
    4 | def test_with_fail_with_reason():
    5 |     karva.fail('This is a custom failure message')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    6 |
    7 | def test_with_fail_with_no_reason():
      |
    info: This is a custom failure message

    error[test-failure]: Test `test_with_fail_with_no_reason` failed
     --> test.py:7:5
      |
    5 |     karva.fail('This is a custom failure message')
    6 |
    7 | def test_with_fail_with_no_reason():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    8 |     karva.fail()
      |
    info: Test failed here
      --> test.py:8:5
       |
     7 | def test_with_fail_with_no_reason():
     8 |     karva.fail()
       |     ^^^^^^^^^^^^
     9 |
    10 | def test_with_fail_with_keyword_reason():
       |

    error[test-failure]: Test `test_with_fail_with_keyword_reason` failed
      --> test.py:10:5
       |
     8 |     karva.fail()
     9 |
    10 | def test_with_fail_with_keyword_reason():
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    11 |     karva.fail(reason='This is a custom failure message')
       |
    info: Test failed here
      --> test.py:11:5
       |
    10 | def test_with_fail_with_keyword_reason():
    11 |     karva.fail(reason='This is a custom failure message')
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
       |
    info: This is a custom failure message

    test result: FAILED. 0 passed; 3 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_conditional_fail ... FAILED

    diagnostics:

    error[test-failure]: Test `test_conditional_fail` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_conditional_fail():
      |     ^^^^^^^^^^^^^^^^^^^^^
    5 |     condition = True
    6 |     if condition:
      |
    info: Test failed here
     --> test.py:7:9
      |
    5 |     condition = True
    6 |     if condition:
    7 |         karva.fail('failing test')
      |         ^^^^^^^^^^^^^^^^^^^^^^^^^^
    8 |     assert True
      |
    info: failing test

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_raise_fail_error ... FAILED

    diagnostics:

    error[test-failure]: Test `test_raise_fail_error` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_raise_fail_error():
      |     ^^^^^^^^^^^^^^^^^^^^^
    5 |     raise karva.FailError('Manually raised FailError')
      |
    info: Test failed here
     --> test.py:5:5
      |
    4 | def test_raise_fail_error():
    5 |     raise karva.FailError('Manually raised FailError')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Manually raised FailError

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

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
        assert_cmd_snapshot!(context.command_no_parallel(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_skip_with_reason ... skipped: This test is skipped at runtime
        test test::test_skip_without_reason ... skipped
        test test::test_conditional_skip ... skipped: Condition was true

        test result: ok. 0 passed; 0 failed; 3 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
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

    assert_cmd_snapshot!(context.command_no_parallel(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_pass ... ok
    test test::test_skip ... skipped: Skipped test
    test test::test_another_pass ... ok

    test result: ok. 2 passed; 0 failed; 1 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_raise_skip_error ... skipped: Manually raised SkipError

    test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_raises_value_error ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_raises_no_exception ... FAILED

    diagnostics:

    error[test-failure]: Test `test_raises_no_exception` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_raises_no_exception():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^
    5 |     with karva.raises(ValueError):
    6 |         pass
      |
    info: Test failed here
     --> test.py:5:5
      |
    4 | def test_raises_no_exception():
    5 |     with karva.raises(ValueError):
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    6 |         pass
      |
    info: DID NOT RAISE <class 'ValueError'>

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_raises_match_passes ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_raises_match_fails ... FAILED

    diagnostics:

    error[test-failure]: Test `test_raises_match_fails` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_raises_match_fails():
      |     ^^^^^^^^^^^^^^^^^^^^^^^
    5 |     with karva.raises(ValueError, match='xyz'):
    6 |         raise ValueError('oops')
      |
    info: Test failed here
     --> test.py:5:5
      |
    4 | def test_raises_match_fails():
    5 |     with karva.raises(ValueError, match='xyz'):
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    6 |         raise ValueError('oops')
      |
    info: Raised exception did not match pattern 'xyz'

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_raises_wrong_type ... FAILED

    diagnostics:

    error[test-failure]: Test `test_raises_wrong_type` failed
     --> test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_raises_wrong_type():
      |     ^^^^^^^^^^^^^^^^^^^^^^
    5 |     with karva.raises(ValueError):
    6 |         raise TypeError('wrong type')
      |
    info: Test failed here
     --> test.py:6:9
      |
    4 | def test_raises_wrong_type():
    5 |     with karva.raises(ValueError):
    6 |         raise TypeError('wrong type')
      |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: wrong type

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_raises_exc_info ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_raises_subclass ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
