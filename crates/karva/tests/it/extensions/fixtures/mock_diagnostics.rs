use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[rstest]
fn last_mock_call_reports_nested_argument_differences(
    #[values(
        "assert_called_with",
        "assert_called_once_with",
        "assert_awaited_with",
        "assert_awaited_once_with"
    )]
    assertion: &str,
) {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os


async def test_arguments(mocker):
    assertion = os.environ["MOCK_ASSERTION"]
    callback = mocker.AsyncMock() if "awaited" in assertion else mocker.Mock()
    if "awaited" in assertion:
        await callback({"users": [{"id": 1}]}, timeout=3)
    else:
        callback({"users": [{"id": 1}]}, timeout=3)
    getattr(callback, assertion)({"users": [{"id": 2}]}, timeout=4)
"#,
    );
    insta::allow_duplicates! {
        if assertion.contains("awaited") {
            assert_cmd_snapshot!(context.command_no_parallel().env("MOCK_ASSERTION", assertion), @r#"
            success: false
            exit_code: 1
            ----- stdout -----
                Starting 1 test across 1 worker
                    FAIL [TIME] test::test_arguments(mocker)

            failures:

            test::test_arguments(mocker):

            error[test-failure]: Test `test_arguments` failed
             --> test.py:5:11
              |
            5 | async def test_arguments(mocker):
              |           ^^^^^^^^^^^^^^
            info: Test ran with arguments:
            info:   `mocker`: `<MockerFixture>`
            info: Test failed here
              --> test.py:12:5
               |
            12 |     getattr(callback, assertion)({"users": [{"id": 2}]}, timeout=4)
               |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
            info: expected await not found.
                  Expected: mock({'users': [{'id': 2}]}, timeout=4)
                    Actual: mock({'users': [{'id': 1}]}, timeout=3)
            info: Mock call arguments differ:
            Positional arguments:
              Actual: ({'users': [{'id': 1}]},)
              Expected: ({'users': [{'id': 2}]},)
            Difference at [0]['users'][0]['id']:
              left: 1
              right: 2
            Keyword arguments:
              Actual: {'timeout': 3}
              Expected: {'timeout': 4}
            Difference at ['timeout']:
              left: 3
              right: 4

            ────────────
                 Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

            ----- stderr -----
            "#);
        } else {
            assert_cmd_snapshot!(context.command_no_parallel().env("MOCK_ASSERTION", assertion), @r#"
            success: false
            exit_code: 1
            ----- stdout -----
                Starting 1 test across 1 worker
                    FAIL [TIME] test::test_arguments(mocker)

            failures:

            test::test_arguments(mocker):

            error[test-failure]: Test `test_arguments` failed
             --> test.py:5:11
              |
            5 | async def test_arguments(mocker):
              |           ^^^^^^^^^^^^^^
            info: Test ran with arguments:
            info:   `mocker`: `<MockerFixture>`
            info: Test failed here
              --> test.py:12:5
               |
            12 |     getattr(callback, assertion)({"users": [{"id": 2}]}, timeout=4)
               |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
            info: expected call not found.
                  Expected: mock({'users': [{'id': 2}]}, timeout=4)
                    Actual: mock({'users': [{'id': 1}]}, timeout=3)
            info: Mock call arguments differ:
            Positional arguments:
              Actual: ({'users': [{'id': 1}]},)
              Expected: ({'users': [{'id': 2}]},)
            Difference at [0]['users'][0]['id']:
              left: 1
              right: 2
            Keyword arguments:
              Actual: {'timeout': 3}
              Expected: {'timeout': 4}
            Difference at ['timeout']:
              left: 3
              right: 4

            ────────────
                 Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

            ----- stderr -----
            "#);
        }
    }
}

#[test]
fn mock_count_mismatch_keeps_original_message_without_argument_diff() {
    let context = TestContext::with_file(
        "test.py",
        r#"
def test_count(mocker):
    callback = mocker.Mock()
    callback("first")
    callback("second")
    callback.assert_called_once_with("expected")
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_count(mocker)

    failures:

    test::test_count(mocker):

    error[test-failure]: Test `test_count` failed
     --> test.py:2:5
      |
    2 | def test_count(mocker):
      |     ^^^^^^^^^^
    info: Test ran with arguments:
    info:   `mocker`: `<MockerFixture>`
    info: Test failed here
     --> test.py:6:5
      |
    6 |     callback.assert_called_once_with("expected")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    info: Expected 'mock' to be called once. Called 2 times.
          Calls: [call('first'), call('second')].

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn caught_mock_assertions_keep_standard_library_exception_text() {
    let context = TestContext::with_file(
        "test.py",
        r#"
from unittest.mock import Mock, AsyncMock
import karva


def test_caught():
    callback = Mock()
    callback("actual")
    with karva.raises(AssertionError) as error:
        callback.assert_called_with("expected")
    assert str(error.value) == "expected call not found.\nExpected: mock('expected')\n  Actual: mock('actual')"
    callback.assert_called_once_with("actual")


async def test_awaited():
    callback = AsyncMock()
    await callback("actual")
    callback.assert_awaited_once_with("actual")
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_caught
            PASS [TIME] test::test_awaited
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}
