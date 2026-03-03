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
    time.sleep(0.05)

def test_slow():
    time.sleep(0.1)
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().args(["--durations", "2"]), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_durations::test_fast ... ok
    test test_durations::test_medium ... ok
    test test_durations::test_slow ... ok

    2 slowest tests:
      test_durations::test_slow ([TIME])
      test_durations::test_medium ([TIME])
    test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel().args(["--durations", "10"]), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_durations::test_fast ... ok
    test test_durations::test_slow ... ok

    2 slowest tests:
      test_durations::test_slow ([TIME])
      test_durations::test_fast ([TIME])
    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

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

    assert_cmd_snapshot!(context.command_no_parallel().args(["--durations", "0"]), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_durations::test_a ... ok

    0 slowest tests:
    test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn durations_with_failing_tests() {
    let context = TestContext::with_file(
        "test_durations.py",
        "def test_pass():\n    pass\n\ndef test_fail():\n    assert False\n",
    );

    assert_cmd_snapshot!(context.command_no_parallel().args(["--durations", "5"]), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_durations::test_pass ... ok
    test test_durations::test_fail ... FAILED

    diagnostics:

    error[test-failure]: Test `test_fail` failed
     --> test_durations.py:4:5
      |
    2 |     pass
    3 |
    4 | def test_fail():
      |     ^^^^^^^^^
    5 |     assert False
      |
    info: Test failed here
     --> test_durations.py:5:5
      |
    4 | def test_fail():
    5 |     assert False
      |     ^^^^^^^^^^^^
      |


    2 slowest tests:
      test_durations::test_fail ([TIME])
      test_durations::test_pass ([TIME])
    test result: FAILED. 1 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
