use insta::{allow_duplicates, assert_snapshot};
use karva_test::TestContext;
use rstest::rstest;

use crate::common::TestRunnerExt;

#[test]
fn test_fail_function() {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    assert_snapshot!(result.display(), @r"
    diagnostics:

    error[test-failure]: Test `test_with_fail_with_reason` failed
     --> <test>/test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_with_fail_with_reason():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^
    5 |     karva.fail('This is a custom failure message')
      |
    info: Test failed here
     --> <test>/test.py:5:5
      |
    4 | def test_with_fail_with_reason():
    5 |     karva.fail('This is a custom failure message')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    6 |
    7 | def test_with_fail_with_no_reason():
      |
    info: This is a custom failure message

    error[test-failure]: Test `test_with_fail_with_no_reason` failed
     --> <test>/test.py:7:5
      |
    5 |     karva.fail('This is a custom failure message')
    6 |
    7 | def test_with_fail_with_no_reason():
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    8 |     karva.fail()
      |
    info: Test failed here
      --> <test>/test.py:8:5
       |
     7 | def test_with_fail_with_no_reason():
     8 |     karva.fail()
       |     ^^^^^^^^^^^^
     9 |
    10 | def test_with_fail_with_keyword_reason():
       |

    error[test-failure]: Test `test_with_fail_with_keyword_reason` failed
      --> <test>/test.py:10:5
       |
     8 |     karva.fail()
     9 |
    10 | def test_with_fail_with_keyword_reason():
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    11 |     karva.fail(reason='This is a custom failure message')
       |
    info: Test failed here
      --> <test>/test.py:11:5
       |
    10 | def test_with_fail_with_keyword_reason():
    11 |     karva.fail(reason='This is a custom failure message')
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
       |
    info: This is a custom failure message

    test result: FAILED. 0 passed; 3 failed; 0 skipped; finished in [TIME]
    ");
}

#[test]
fn test_fail_function_conditional() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import karva

def test_conditional_fail():
    condition = True
    if condition:
        karva.fail('failing test')
    assert True
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r"
    diagnostics:

    error[test-failure]: Test `test_conditional_fail` failed
     --> <test>/test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_conditional_fail():
      |     ^^^^^^^^^^^^^^^^^^^^^
    5 |     condition = True
    6 |     if condition:
      |
    info: Test failed here
     --> <test>/test.py:7:9
      |
    5 |     condition = True
    6 |     if condition:
    7 |         karva.fail('failing test')
      |         ^^^^^^^^^^^^^^^^^^^^^^^^^^
    8 |     assert True
      |
    info: failing test

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    ");
}

#[test]
fn test_fail_error_exception() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import karva

def test_raise_fail_error():
    raise karva.FailError('Manually raised FailError')
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r"
    diagnostics:

    error[test-failure]: Test `test_raise_fail_error` failed
     --> <test>/test.py:4:5
      |
    2 | import karva
    3 |
    4 | def test_raise_fail_error():
      |     ^^^^^^^^^^^^^^^^^^^^^
    5 |     raise karva.FailError('Manually raised FailError')
      |
    info: Test failed here
     --> <test>/test.py:5:5
      |
    4 | def test_raise_fail_error():
    5 |     raise karva.FailError('Manually raised FailError')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: Manually raised FailError

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    ");
}

#[rstest]
fn test_runtime_skip_pytest(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 3 skipped; finished in [TIME]");
    }
}

#[test]
fn test_mixed_skip_and_pass() {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 1 skipped; finished in [TIME]");
}

#[test]
fn test_skip_error_exception() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import karva

def test_raise_skip_error():
    raise karva.SkipError('Manually raised SkipError')
    assert False, 'This should not be reached'
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
}
