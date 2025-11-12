use insta::{allow_duplicates, assert_snapshot};
use karva_test::TestContext;
use rstest::rstest;

use crate::common::{TestRunnerExt, get_skipif_function};

#[rstest]
fn test_skipif_true_condition(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test_skipif.py",
        &format!(
            r"
import {framework}

@{decorator}(True, reason='Condition is true')
def test_1():
    assert False

        ",
            decorator = get_skipif_function(framework)
        ),
    );

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skipif_false_condition(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test_skipif.py",
        &format!(
            r"
import {framework}

@{decorator}(False, reason='Condition is false')
def test_1():
    assert True
        ",
            decorator = get_skipif_function(framework)
        ),
    );

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skipif_expression(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test_skipif.py",
        &format!(
            r"
import {framework}
import sys

@{decorator}(sys.version_info >= (3, 0), reason='Python 3 or higher')
def test_1():
    assert False
        ",
            decorator = get_skipif_function(framework)
        ),
    );

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skipif_multiple_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test_skipif.py",
        &format!(
            r"
import {framework}

@{decorator}(True, False, reason='Multiple conditions with one true')
def test_1():
    assert False
        ",
            decorator = get_skipif_function(framework)
        ),
    );

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skipif_without_reason(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test_skipif.py",
        &format!(
            r"
import {framework}

@{decorator}(True)
def test_1():
    assert False
        ",
            decorator = get_skipif_function(framework)
        ),
    );

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skipif_multiple_tests(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test_skipif.py",
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
            decorator = get_skipif_function(framework)
        ),
    );

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 1 skipped; finished in [TIME]");
    }
}

#[rstest]
fn test_skipif_all_false_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "<test>/test_skipif.py",
        &format!(
            r"
import {framework}

@{decorator}(False, False, reason='All conditions false')
def test_1():
    assert True
        ",
            decorator = get_skipif_function(framework)
        ),
    );

    let result = context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}
