use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn function_fixture_teardown_crash_is_attributed_to_active_test() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os

import karva


@karva.fixture
def crash_during_teardown():
    yield
    os._exit(43)


def test_crash(crash_during_teardown):
    pass
",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           CRASH [TIME] test::test_crash(crash_during_teardown=None)

    failures:

    test::test_crash(crash_during_teardown=None):

    error[worker-crashed]: Worker terminated with exit code 43 while running `test::test_crash(crash_during_teardown=None)`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 43 in [TIME]
    ");
}
