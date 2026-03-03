use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn last_failed_reruns_only_failures() {
    let context = TestContext::with_files([(
        "test_a.py",
        "
            def test_pass(): pass
            def test_fail(): assert False
            ",
    )]);

    context.command_no_parallel().output().unwrap();

    assert_cmd_snapshot!(context.command_no_parallel().arg("--last-failed"), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_a::test_fail ... FAILED

    diagnostics:

    error[test-failure]: Test `test_fail` failed
     --> test_a.py:3:5
      |
    2 | def test_pass(): pass
    3 | def test_fail(): assert False
      |     ^^^^^^^^^
      |
    info: Test failed here
     --> test_a.py:3:1
      |
    2 | def test_pass(): pass
    3 | def test_fail(): assert False
      | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn last_failed_lf_alias() {
    let context = TestContext::with_files([(
        "test_a.py",
        "
            def test_pass(): pass
            def test_fail(): assert False
            ",
    )]);

    context.command_no_parallel().output().unwrap();

    assert_cmd_snapshot!(context.command_no_parallel().arg("--lf"), @r"
    success: false
    exit_code: 1
    ----- stdout -----
    test test_a::test_fail ... FAILED

    diagnostics:

    error[test-failure]: Test `test_fail` failed
     --> test_a.py:3:5
      |
    2 | def test_pass(): pass
    3 | def test_fail(): assert False
      |     ^^^^^^^^^
      |
    info: Test failed here
     --> test_a.py:3:1
      |
    2 | def test_pass(): pass
    3 | def test_fail(): assert False
      | ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |

    test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn last_failed_with_no_previous_failures_runs_all() {
    let context = TestContext::with_files([(
        "test_a.py",
        "
            def test_one(): pass
            def test_two(): pass
            ",
    )]);

    context.command_no_parallel().output().unwrap();

    assert_cmd_snapshot!(context.command_no_parallel().arg("--last-failed"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_a::test_one ... ok
    test test_a::test_two ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}

#[test]
fn last_failed_without_previous_run_runs_all() {
    let context = TestContext::with_files([(
        "test_a.py",
        "
            def test_one(): pass
            def test_two(): pass
            ",
    )]);

    assert_cmd_snapshot!(context.command_no_parallel().arg("--last-failed"), @r"
    success: true
    exit_code: 0
    ----- stdout -----
    test test_a::test_one ... ok
    test test_a::test_two ... ok

    test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]

    ----- stderr -----
    ");
}
