use insta::{allow_duplicates, assert_snapshot};
use karva_test::TestContext;
use rstest::rstest;

use crate::common::{TestRunnerExt, get_skip_function};

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
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skip_keyword(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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
    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skip_functionality_no_reason(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skip_reason_function_call(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skip_with_true_condition(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skip_with_false_condition(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skip_with_expression(#[values("pytest", "karva")] framework: &str) {
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
            decorator = get_skip_decorator(framework)
        ),
    );

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skip_with_multiple_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skip_with_condition_without_reason(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skip_with_multiple_tests(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skip_with_all_false_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test.py",
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

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[test]
fn test_skip_with_empty_conditions_karva() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import karva

@karva.tags.skip()
def test_1():
    assert False
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
}

#[test]
fn test_skip_with_single_string_as_reason_karva() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import karva

@karva.tags.skip('This is the skip reason')
def test_1():
    assert False
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
}

#[test]
fn test_skip_with_invalid_condition_integer_karva() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import karva

@karva.tags.skip(1, 0, reason='Invalid integer conditions')
def test_1():
    assert True
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r"
    discovery diagnostics:

    error[failed-to-import-module]: Failed to import python module `<test>.test`

    test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]
    ");
}

#[test]
fn test_skip_with_mixed_valid_invalid_conditions_karva() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import karva

@karva.tags.skip(True, 'false', reason='Mixed valid and invalid')
def test_1():
    assert True
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r"
    discovery diagnostics:

    error[failed-to-import-module]: Failed to import python module `<test>.test`

    test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]
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
