use insta::assert_snapshot;
use karva_test::TestContext;

use crate::common::TestRunnerExt;

#[test]
fn test_invalid_pytest_fixture_scope() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r#"
                import pytest

                @pytest.fixture(scope="sessionss")
                def some_fixture() -> int:
                    return 1

                def test_all_scopes(
                    some_fixture: int,
                ) -> None:
                    assert some_fixture == 1
                "#,
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r#"
    discovery failures:

    invalid fixture `some_fixture` at <test>/test.py:4: Failed to parse fixture

    test failures:

    test `<test>.test::test_all_scopes` has missing fixtures: ["some_fixture"] at <test>/test.py:8

    test failures:
        <test>.test::test_all_scopes at <test>/test.py:8

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    "#);

    assert!(result.total_diagnostics() == 2);
}

#[test]
fn test_missing_fixture() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
                def test_all_scopes(
                    missing_fixture: int,
                ) -> None:
                    assert missing_fixture == 1
                ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r#"
    test failures:

    test `<test>.test::test_all_scopes` has missing fixtures: ["missing_fixture"] at <test>/test.py:2

    test failures:
        <test>.test::test_all_scopes at <test>/test.py:2

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    "#);

    assert!(result.diagnostics().len() == 1);
}

#[test]
fn test_fixture_fails_to_run() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
                from karva import fixture

                @fixture
                def failing_fixture():
                    raise Exception('Fixture failed')

                def test_failing_fixture(failing_fixture):
                    pass
                ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r#"
    fixture failures:

    fixture function `<test>.test::failing_fixture` at <test>/test.py:4 failed at <test>/test.py:6
    Fixture failed

    test failures:

    test `<test>.test::test_failing_fixture` has missing fixtures: ["failing_fixture"] at <test>/test.py:8

    test failures:
        <test>.test::test_failing_fixture at <test>/test.py:8

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    "#);
}

#[test]
fn test_fixture_missing_fixtures() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
                from karva import fixture

                @fixture
                def failing_fixture(missing_fixture):
                    return 1

                def test_failing_fixture(failing_fixture):
                    pass
                ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r#"
    fixture failures:

    fixture `<test>.test::failing_fixture` has missing fixtures: ["missing_fixture"] at <test>/test.py:4

    test failures:

    test `<test>.test::test_failing_fixture` has missing fixtures: ["failing_fixture"] at <test>/test.py:8

    test failures:
        <test>.test::test_failing_fixture at <test>/test.py:8

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    "#);
}

#[test]
fn missing_arguments_in_nested_function() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
                def test_failing_fixture():

                    def inner(missing_fixture): ...

                    inner()
                   ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @r"
    test failures:

    test `<test>.test::test_failing_fixture` at <test>/test.py:2 failed at <test>/test.py:6
    test_failing_fixture.<locals>.inner() missing 1 required positional argument: 'missing_fixture'
    note: run with `--show-traceback` to see the full traceback

    test failures:
        <test>.test::test_failing_fixture at <test>/test.py:2

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    ");
}
