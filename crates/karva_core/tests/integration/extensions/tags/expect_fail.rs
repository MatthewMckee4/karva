use insta::{allow_duplicates, assert_snapshot};
use karva_test::TestContext;
use rstest::rstest;

use crate::common::TestRunnerExt;

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
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_expect_fail_that_passes_karva() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected to fail but passes')
def test_1():
    assert True
        ",
    );

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @r"
        diagnostics:

        error[test-pass-on-expect-failure]: Test `test_1` passes when expected to fail
         --> <test>/test.py:5:5
          |
        4 | @karva.tags.expect_fail(reason='Expected to fail but passes')
        5 | def test_1():
          |     ^^^^^^
        6 |     assert True
          |
        info: Reason: Expected to fail but passes

        test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
        ");
    }
}

#[rstest]
fn test_expect_fail_that_passes_pytest() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import pytest

@pytest.mark.xfail(reason='Expected to fail but passes')
def test_1():
    assert True
        ",
    );

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @r"
        diagnostics:

        error[test-pass-on-expect-failure]: Test `test_1` passes when expected to fail
         --> <test>/test.py:5:5
          |
        4 | @pytest.mark.xfail(reason='Expected to fail but passes')
        5 | def test_1():
          |     ^^^^^^
        6 |     assert True
          |
        info: Reason: Expected to fail but passes

        test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
        ");
    }
}

#[rstest]
fn test_expect_fail_no_reason(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_expect_fail_with_call(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_expect_fail_with_true_condition(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_expect_fail_with_false_condition(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_expect_fail_with_expression(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_expect_fail_with_multiple_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_expect_fail_with_all_false_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[test]
fn test_expect_fail_with_single_string_as_reason_karva() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import karva

@karva.tags.expect_fail('This is expected to fail')
def test_1():
    assert False
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_expect_fail_with_empty_conditions_karva() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import karva

@karva.tags.expect_fail()
def test_1():
    assert False
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[rstest]
fn test_expect_fail_mixed_tests_karva() {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @r"
        diagnostics:

        error[test-pass-on-expect-failure]: Test `test_expected_fail_passes` passes when expected to fail
          --> <test>/test.py:12:5
           |
        11 | @karva.tags.expect_fail()
        12 | def test_expected_fail_passes():
           |     ^^^^^^^^^^^^^^^^^^^^^^^^^
        13 |     assert True
           |

        test result: FAILED. 2 passed; 1 failed; 0 skipped; finished in [TIME]
        ");
    }
}

#[rstest]
fn test_expect_fail_mixed_tests_pytest() {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @r"
        diagnostics:

        error[test-pass-on-expect-failure]: Test `test_expected_fail_passes` passes when expected to fail
          --> <test>/test.py:12:5
           |
        11 | @pytest.mark.xfail
        12 | def test_expected_fail_passes():
           |     ^^^^^^^^^^^^^^^^^^^^^^^^^
        13 |     assert True
           |

        test result: FAILED. 2 passed; 1 failed; 0 skipped; finished in [TIME]
        ");
    }
}

#[test]
fn test_expect_fail_with_runtime_error() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected to fail with runtime error')
def test_1():
    raise RuntimeError('Something went wrong')
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_expect_fail_with_assertion_error() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected to fail')
def test_1():
    raise AssertionError('This assertion should fail')
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_expect_fail_with_skip() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected to fail')
def test_1():
    karva.skip('Skipping this test')
    assert False
        ",
    );

    let result = context.test();

    // Skip takes precedence - test should be skipped, not treated as expected fail
    assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
}

#[test]
fn test_expect_fail_then_unexpected_pass() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import karva

@karva.tags.expect_fail(reason='This should fail but passes')
def test_should_fail():
    assert 1 + 1 == 2
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r"
    diagnostics:

    error[test-pass-on-expect-failure]: Test `test_should_fail` passes when expected to fail
     --> <test>/test.py:5:5
      |
    4 | @karva.tags.expect_fail(reason='This should fail but passes')
    5 | def test_should_fail():
      |     ^^^^^^^^^^^^^^^^
    6 |     assert 1 + 1 == 2
      |
    info: Reason: This should fail but passes

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    ");
}
