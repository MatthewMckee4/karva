use insta::{allow_duplicates, assert_snapshot};
use karva_test::TestContext;
use rstest::rstest;

use crate::common::{TestRunnerExt, get_skip_function};

#[rstest]
fn test_skip(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
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

    let result = test_context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skip_keyword(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
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
    let result = test_context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skip_functionality_no_reason(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
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

    let result = test_context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skip_reason_function_call(#[values("pytest", "karva")] framework: &str) {
    let test_context = TestContext::with_file(
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

    let result = test_context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}
