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
        "<test>/test_expect_fail.py",
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
fn test_expect_fail_that_passes(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test_expect_fail.py",
        &format!(
            r"
import {framework}

@{decorator}(reason='Expected to fail but passes')
def test_1():
    assert True
        ",
            decorator = get_expect_fail_decorator(framework)
        ),
    );

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @r"
        test failures:

        test `<test>.test_expect_fail::test_1` at <temp_dir>/<test>/test_expect_fail.py:4 passed when it was expected to fail
        reason: Expected to fail but passes

        test failures:
            <test>.test_expect_fail::test_1 at <temp_dir>/<test>/test_expect_fail.py:4

        test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
        ");
    }
}

#[rstest]
fn test_expect_fail_no_reason(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test_expect_fail.py",
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
        "<test>/test_expect_fail.py",
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
        "<test>/test_expect_fail.py",
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
        "<test>/test_expect_fail.py",
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
        "<test>/test_expect_fail.py",
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
        "<test>/test_expect_fail.py",
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
        "<test>/test_expect_fail.py",
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
        "<test>/test_expect_fail.py",
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
        "<test>/test_expect_fail.py",
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
fn test_expect_fail_mixed_tests(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test_expect_fail.py",
        &format!(
            r"
import {framework}

@{decorator}(reason='Expected to fail')
def test_expected_to_fail():
    assert False

def test_normal_pass():
    assert True

@{decorator}
def test_expected_fail_passes():
    assert True
        ",
            decorator = get_expect_fail_decorator(framework)
        ),
    );

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @r"
        test failures:

        test `<test>.test_expect_fail::test_expected_fail_passes` at <temp_dir>/<test>/test_expect_fail.py:11 passed when it was expected to fail

        test failures:
            <test>.test_expect_fail::test_expected_fail_passes at <temp_dir>/<test>/test_expect_fail.py:11

        test result: FAILED. 2 passed; 1 failed; 0 skipped; finished in [TIME]
        ");
    }
}

#[test]
fn test_expect_fail_with_runtime_error() {
    let context = TestContext::with_file(
        "<test>/test_expect_fail.py",
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
        "<test>/test_expect_fail.py",
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
fn test_fail_function() {
    let context = TestContext::with_file(
        "<test>/test_fail.py",
        r"
import karva

def test_with_fail():
    karva.fail('This is a custom failure message')

def test_normal():
    assert True
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r"
    test failures:

    test `<test>.test_fail::test_with_fail` at <temp_dir>/<test>/test_fail.py:4 failed at <temp_dir>/<test>/test_fail.py:5
    This is a custom failure message
    note: run with `--show-traceback` to see the full traceback

    test failures:
        <test>.test_fail::test_with_fail at <temp_dir>/<test>/test_fail.py:4

    test result: FAILED. 1 passed; 1 failed; 0 skipped; finished in [TIME]
    ");
}

#[test]
fn test_fail_function_conditional() {
    let context = TestContext::with_file(
        "<test>/test_fail.py",
        r"
import karva

def test_conditional_fail():
    condition = True
    if condition:
        karva.fail('Condition was true, failing test')
    assert True
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r"
    test failures:

    test `<test>.test_fail::test_conditional_fail` at <temp_dir>/<test>/test_fail.py:4 failed at <temp_dir>/<test>/test_fail.py:7
    Condition was true, failing test
    note: run with `--show-traceback` to see the full traceback

    test failures:
        <test>.test_fail::test_conditional_fail at <temp_dir>/<test>/test_fail.py:4

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    ");
}

#[test]
fn test_fail_error_exception() {
    let context = TestContext::with_file(
        "<test>/test_fail.py",
        r"
import karva

def test_raise_fail_error():
    raise karva.FailError('Manually raised FailError')
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r"
    test failures:

    test `<test>.test_fail::test_raise_fail_error` at <temp_dir>/<test>/test_fail.py:4 failed at <temp_dir>/<test>/test_fail.py:5
    Manually raised FailError
    note: run with `--show-traceback` to see the full traceback

    test failures:
        <test>.test_fail::test_raise_fail_error at <temp_dir>/<test>/test_fail.py:4

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    ");
}

#[test]
fn test_expect_fail_with_skip() {
    let context = TestContext::with_file(
        "<test>/test_expect_fail.py",
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
        "<test>/test_expect_fail.py",
        r"
import karva

@karva.tags.expect_fail(reason='This should fail but passes')
def test_should_fail():
    assert 1 + 1 == 2
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r"
    test failures:

    test `<test>.test_expect_fail::test_should_fail` at <temp_dir>/<test>/test_expect_fail.py:4 passed when it was expected to fail
    reason: This should fail but passes

    test failures:
        <test>.test_expect_fail::test_should_fail at <temp_dir>/<test>/test_expect_fail.py:4

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    ");
}
