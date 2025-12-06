use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

fn get_skip_function(framework: &str) -> &str {
    match framework {
        "pytest" => "pytest.mark.skip",
        "karva" => "karva.tags.skip",
        _ => panic!("Invalid framework"),
    }
}

fn get_skip_decorator(framework: &str) -> &str {
    match framework {
        "pytest" => "pytest.mark.skipif",
        "karva" => "karva.tags.skip",
        _ => panic!("Invalid framework"),
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_1 ... skipped: This test is skipped with decorator

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_keyword(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_1 ... skipped: This test is skipped with decorator

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_functionality_no_reason(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_1 ... skipped

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_reason_function_call(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_1 ... skipped

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_with_true_condition(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_1 ... skipped: Condition is true

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_with_false_condition(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_1 ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_with_expression(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_1 ... skipped: Python 3 or higher

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_with_multiple_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_1 ... skipped: Multiple conditions with one true

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_with_condition_without_reason(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_1 ... skipped

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_with_multiple_tests(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_skip_this ... skipped: Should skip
        test test::test_run_this ... ok
        test test::test_normal ... ok

        test result: ok. 2 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_with_all_false_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_1 ... ok

        test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_with_empty_conditions_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.skip()
def test_1():
    assert False
        ",
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_1 ... skipped

    test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_with_single_string_as_reason_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.skip('This is the skip reason')
def test_1():
    assert False
        ",
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_1 ... skipped: This is the skip reason

    test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_with_invalid_condition_integer_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.skip(1, 0, reason='Invalid integer conditions')
def test_1():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    discovery diagnostics:

    error[failed-to-import-module]: Failed to import python module `test`: Expected boolean values for conditions

    test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skip_with_mixed_valid_invalid_conditions_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.skip(True, 'false', reason='Mixed valid and invalid')
def test_1():
    assert True
        ",
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    discovery diagnostics:

    error[failed-to-import-module]: Failed to import python module `test`: Expected boolean values for conditions

    test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_skipif_true_and_false_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
        &format!(
            r"
import {framework}

@{decorator}(True)
@{decorator}(False)
def test_skip_with_true():
    assert False

        ",
            decorator = get_skip_decorator(framework)
        ),
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        test test::test_skip_with_true ... skipped

        test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}
