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
    diagnostics:

    invalid-fixture: Discovered an invalid fixture `some_fixture`
     --> <test>/test.py:5:5
      |
    4 | @pytest.fixture(scope="sessionss")
    5 | def some_fixture() -> int:
      |     ^^^^^^^^^^^^
    6 |     return 1
      |
    info: Reason: Failed to parse fixture

    missing-fixtures: Discovered missing fixtures for test `test_all_scopes`
      --> <test>/test.py:8:5
       |
     6 |     return 1
     7 |
     8 | def test_all_scopes(
       |     ^^^^^^^^^^^^^^^
     9 |     some_fixture: int,
    10 | ) -> None:
       |
    info: Missing fixtures: ["some_fixture"]

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
    diagnostics:

    missing-fixtures: Discovered missing fixtures for test `test_all_scopes`
     --> <test>/test.py:2:5
      |
    2 | def test_all_scopes(
      |     ^^^^^^^^^^^^^^^
    3 |     missing_fixture: int,
    4 | ) -> None:
      |
    info: Missing fixtures: ["missing_fixture"]

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
    diagnostics:

    fixture-failure: Fixture `failing_fixture` failed
     --> <test>/test.py:5:5
      |
    4 | @fixture
    5 | def failing_fixture():
      |     ^^^^^^^^^^^^^^^
    6 |     raise Exception('Fixture failed')
      |
    info: Test failed here
     --> <test>/test.py:6:5
      |
    4 | @fixture
    5 | def failing_fixture():
    6 |     raise Exception('Fixture failed')
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    7 |
    8 | def test_failing_fixture(failing_fixture):
      |
    info: Error message: Fixture failed

    missing-fixtures: Discovered missing fixtures for test `test_failing_fixture`
     --> <test>/test.py:8:5
      |
    6 |     raise Exception('Fixture failed')
    7 |
    8 | def test_failing_fixture(failing_fixture):
      |     ^^^^^^^^^^^^^^^^^^^^
    9 |     pass
      |
    info: Missing fixtures: ["failing_fixture"]

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
    diagnostics:

    missing-fixtures: Discovered missing fixtures for fixture `failing_fixture`
     --> <test>/test.py:5:5
      |
    4 | @fixture
    5 | def failing_fixture(missing_fixture):
      |     ^^^^^^^^^^^^^^^
    6 |     return 1
      |
    info: Missing fixtures: ["missing_fixture"]

    missing-fixtures: Discovered missing fixtures for test `test_failing_fixture`
     --> <test>/test.py:8:5
      |
    6 |     return 1
    7 |
    8 | def test_failing_fixture(failing_fixture):
      |     ^^^^^^^^^^^^^^^^^^^^
    9 |     pass
      |
    info: Missing fixtures: ["failing_fixture"]

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
    diagnostics:

    test-failure: Test `test_failing_fixture` failed
     --> <test>/test.py:2:5
      |
    2 | def test_failing_fixture():
      |     ^^^^^^^^^^^^^^^^^^^^
    3 |
    4 |     def inner(missing_fixture): ...
      |
    info: Test failed here
     --> <test>/test.py:6:5
      |
    4 |     def inner(missing_fixture): ...
    5 |
    6 |     inner()
      |     ^^^^^^^
      |
    info: Error message: test_failing_fixture.<locals>.inner() missing 1 required positional argument: 'missing_fixture'

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]
    ");
}
