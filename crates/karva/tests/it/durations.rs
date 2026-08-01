use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn durations_shows_slowest_tests() {
    let context = TestContext::with_file(
        "test_durations.py",
        r"
import time

def test_fast():
    pass

def test_medium():
    time.sleep(0.2)

def test_slow():
    time.sleep(0.5)
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().args(["--durations", "2"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test_durations::test_fast
            PASS [TIME] test_durations::test_medium
            PASS [TIME] test_durations::test_slow

    2 slowest tests:
      test_durations::test_slow ([TIME])
      test_durations::test_medium ([TIME])

    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn durations_shows_all_when_n_exceeds_test_count() {
    let context = TestContext::with_file(
        "test_durations.py",
        r"
import time

def test_fast():
    pass

def test_slow():
    time.sleep(0.05)
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().args(["--durations", "10"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_durations::test_fast
            PASS [TIME] test_durations::test_slow

    2 slowest tests:
      test_durations::test_slow ([TIME])
      test_durations::test_fast ([TIME])

    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn durations_zero_shows_header_only() {
    let context = TestContext::with_file(
        "test_durations.py",
        r"
def test_a():
    pass
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().args(["--durations", "0"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_durations::test_a
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn durations_with_skipped_tests() {
    let context = TestContext::with_file(
        "test_durations.py",
        r"
import karva

def test_pass():
    pass

@karva.tags.skip
def test_skipped():
    pass
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().args(["--durations", "5"]), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_durations::test_pass

    2 slowest tests:
      test_durations::test_pass ([TIME])
      test_durations::test_skipped ([TIME])

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 skipped

    ----- stderr -----
    ");
}

#[test]
fn durations_with_failing_tests() {
    // `test_fail` sleeps so it is reliably the slower of the two tests; without
    // a sleep both tests run in microseconds and the slowest-tests ordering is
    // non-deterministic across machines (especially noticeable in CI).
    let context = TestContext::with_file(
        "test_durations.py",
        "import time\n\ndef test_pass():\n    pass\n\ndef test_fail():\n    time.sleep(0.05)\n    assert False\n",
    );

    assert_cmd_snapshot!(context.command_no_parallel().args(["--durations", "5"]), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test_durations::test_pass
            FAIL [TIME] test_durations::test_fail

    failures:

    test_durations::test_fail:

    error[test-failure]: Test `test_fail` failed
     --> test_durations.py:6:5
      |
    6 | def test_fail():
      |     ^^^^^^^^^
      |
    info: Test failed here
     --> test_durations.py:8:5
      |
    8 |     assert False
      |     ^^^^^^^^^^^^
      |
    info: assert False

    2 slowest tests:
      test_durations::test_fail ([TIME])
      test_durations::test_pass ([TIME])

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}
