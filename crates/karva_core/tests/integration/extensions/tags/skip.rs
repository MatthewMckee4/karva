use insta::{allow_duplicates, assert_snapshot};
use karva_test::TestContext;
use rstest::rstest;

use crate::common::{TestRunnerExt, get_skip_function};

#[rstest]
fn test_skip(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test_skip.py",
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
        "<test>/test_skip.py",
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
        "<test>/test_skip.py",
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
        "<test>/test_skip.py",
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

#[test]
fn test_runtime_skip_karva() {
    let context = TestContext::with_file(
        "<test>/test_runtime_skip.py",
        r"
import karva

def test_skip_with_reason():
    karva.skip('This test is skipped at runtime')
    assert False, 'This should not be reached'

def test_skip_without_reason():
    karva.skip()
    assert False, 'This should not be reached'

def test_conditional_skip():
    condition = True
    if condition:
        karva.skip('Condition was true')
    assert False, 'This should not be reached'
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 3 skipped; finished in [TIME]");
}

#[rstest]
fn test_runtime_skip_pytest(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test_pytest_skip.py",
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
        "<test>/test_mixed.py",
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
        "<test>/test_skip_error.py",
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
