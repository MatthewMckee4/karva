use insta::allow_duplicates;
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

fn get_expect_fail_decorator(framework: &str) -> &str {
    match framework {
        "pytest" => "pytest.mark.xfail",
        "karva" => "karva.tags.expect_fail",
        _ => panic!("Invalid framework"),
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_that_fails(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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
fn test_expect_fail_that_passes_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected to fail but passes')
def test_1():
    assert True
        ",
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: false
        exit_code: 1
        ----- stdout -----
        test test::test_1 ... FAILED

        diagnostics:

        error[test-pass-on-expect-failure]: Test `test_1` passes when expected to fail
         --> test.py:5:5
          |
        4 | @karva.tags.expect_fail(reason='Expected to fail but passes')
        5 | def test_1():
          |     ^^^^^^
        6 |     assert True
          |
        info: Reason: Expected to fail but passes

        test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_that_passes_pytest() {
    let context = TestContext::with_file(
        "test.py",
        r"
import pytest

@pytest.mark.xfail(reason='Expected to fail but passes')
def test_1():
    assert True
        ",
    );

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: false
        exit_code: 1
        ----- stdout -----
        test test::test_1 ... FAILED

        diagnostics:

        error[test-pass-on-expect-failure]: Test `test_1` passes when expected to fail
         --> test.py:5:5
          |
        4 | @pytest.mark.xfail(reason='Expected to fail but passes')
        5 | def test_1():
          |     ^^^^^^
        6 |     assert True
          |
        info: Reason: Expected to fail but passes

        test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_no_reason(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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
fn test_expect_fail_with_call(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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
fn test_expect_fail_with_true_condition(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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
fn test_expect_fail_with_false_condition(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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
fn test_expect_fail_with_expression(#[values("pytest", "karva")] framework: &str) {
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
            decorator = get_expect_fail_decorator(framework)
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
fn test_expect_fail_with_multiple_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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
fn test_expect_fail_with_all_false_conditions(#[values("pytest", "karva")] framework: &str) {
    let context = TestContext::with_file(
        "test.py",
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
fn test_expect_fail_with_single_string_as_reason_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail('This is expected to fail')
def test_1():
    assert False
        ",
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_1 ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_with_empty_conditions_karva() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail()
def test_1():
    assert False
        ",
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_1 ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_mixed_tests_karva() {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: false
        exit_code: 1
        ----- stdout -----
        test test::test_expected_to_fail ... ok
        test test::test_normal_pass ... ok
        test test::test_expected_fail_passes ... FAILED

        diagnostics:

        error[test-pass-on-expect-failure]: Test `test_expected_fail_passes` passes when expected to fail
          --> test.py:12:5
           |
        11 | @karva.tags.expect_fail()
        12 | def test_expected_fail_passes():
           |     ^^^^^^^^^^^^^^^^^^^^^^^^^
        13 |     assert True
           |

        test result: FAILED. 2 passed; 1 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[rstest]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_mixed_tests_pytest() {
    let context = TestContext::with_file(
        "test.py",
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

    allow_duplicates! {
        assert_cmd_snapshot!(context.command(), @r"
        success: false
        exit_code: 1
        ----- stdout -----
        test test::test_expected_to_fail ... ok
        test test::test_normal_pass ... ok
        test test::test_expected_fail_passes ... FAILED

        diagnostics:

        error[test-pass-on-expect-failure]: Test `test_expected_fail_passes` passes when expected to fail
          --> test.py:12:5
           |
        11 | @pytest.mark.xfail
        12 | def test_expected_fail_passes():
           |     ^^^^^^^^^^^^^^^^^^^^^^^^^
        13 |     assert True
           |

        test result: FAILED. 2 passed; 1 failed; 0 skipped; finished in [TIME]

        ----- stderr -----
        ");
    }
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_with_runtime_error() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected to fail with runtime error')
def test_1():
    raise RuntimeError('Something went wrong')
        ",
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_1 ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_with_assertion_error() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected to fail')
def test_1():
    raise AssertionError('This assertion should fail')
        ",
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_1 ... ok

    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_with_skip() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail(reason='Expected to fail')
def test_1():
    karva.skip('Skipping this test')
    assert False
        ",
    );

    // Skip takes precedence - test should be skipped, not treated as expected fail
    assert_cmd_snapshot!(context.command(), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test::test_1 ... skipped: Skipping this test

    test result: ok. 0 passed; 0 failed; 1 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
#[ignore = "Will fail unless `maturin build` is ran"]
fn test_expect_fail_then_unexpected_pass() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva

@karva.tags.expect_fail(reason='This should fail but passes')
def test_should_fail():
    assert 1 + 1 == 2
        ",
    );

    assert_cmd_snapshot!(context.command(), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test::test_should_fail ... FAILED

    diagnostics:

    error[test-pass-on-expect-failure]: Test `test_should_fail` passes when expected to fail
     --> test.py:5:5
      |
    4 | @karva.tags.expect_fail(reason='This should fail but passes')
    5 | def test_should_fail():
      |     ^^^^^^^^^^^^^^^^
    6 |     assert 1 + 1 == 2
      |
    info: Reason: This should fail but passes

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
