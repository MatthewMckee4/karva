use insta::assert_snapshot;
use insta_cmd::assert_cmd_snapshot;
use serde_json::Value;

use crate::common::TestContext;

#[test]
fn writes_json_result_report() {
    let context = TestContext::with_file(
        "test_report.py",
        r#"
        import pathlib
        import sys

        import karva

        def test_pass():
            print("pass stdout")

        def test_fail():
            print("fail stderr", file=sys.stderr)
            assert False

        def test_error(missing_fixture):
            pass

        @karva.tags.skip("skip reason")
        def test_skip():
            assert False

        def test_flaky():
            marker = pathlib.Path("flaky-count.txt")
            count = int(marker.read_text()) if marker.exists() else 0
            marker.write_text(str(count + 1))
            print(f"flaky attempt {count + 1}")
            assert count >= 1
        "#,
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--retry=1",
            "--color=always",
            "--status-level=none",
            "--result-output=reports/results.json",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_report::test_error:

    error[missing-fixtures]: Test `test_error` has missing fixtures
      --> test_report.py:14:5
       |
    14 | def test_error(missing_fixture):
       |     ^^^^^^^^^^
       |
    info: Missing fixtures: `missing_fixture`

    test_report::test_fail:

    error[test-failure]: Test `test_fail` failed
      --> test_report.py:10:5
       |
    10 | def test_fail():
       |     ^^^^^^^^^
       |
    info: Test failed here
      --> test_report.py:12:5
       |
    12 |     assert False
       |     ^^^^^^^^^^^^
       |

    captured stderr:
    fail stderr
    fail stderr

    ────────────
         Summary [TIME] 5 tests run: 2 passed (1 flaky), 1 failed, 1 error, 1 skipped
       FLAKY 2/2 [TIME] test_report::test_flaky

    ----- stderr -----
    "
    );

    let report = context.read_file("reports/results.json");
    assert!(!report.contains('\u{1b}'));
    assert_snapshot!(report, @r#"
    {
      "schema_version": 2,
      "status": "failed",
      "elapsed_seconds": "[TIME]",
      "stats": {
        "total": 5,
        "passed": 2,
        "failed": 1,
        "errors": 1,
        "skipped": 1,
        "flaky": 1,
        "slow": 0
      },
      "tests": [
        {
          "module": "test_report",
          "name": "test_error",
          "full_name": "test_report::test_error",
          "status": "error",
          "duration_seconds": "[TIME]",
          "diagnostic": {
            "code": "missing-fixtures",
            "severity": "error",
            "message": "Test `test_error` has missing fixtures",
            "rendered": "error[missing-fixtures]: Test `test_error` has missing fixtures\n  --> test_report.py:14:5\n   |/n14 | def test_error(missing_fixture):\n   |     ^^^^^^^^^^\n   |/ninfo: Missing fixtures: `missing_fixture`\n\n"
          }
        },
        {
          "module": "test_report",
          "name": "test_fail",
          "full_name": "test_report::test_fail",
          "status": "failed",
          "duration_seconds": "[TIME]",
          "retry": {
            "attempts": 2,
            "max_attempts": 2
          },
          "captured_output": {
            "stderr": "fail stderr/nfail stderr\n"
          },
          "diagnostic": {
            "code": "test-failure",
            "severity": "error",
            "message": "Test `test_fail` failed",
            "rendered": "error[test-failure]: Test `test_fail` failed\n  --> test_report.py:10:5\n   |/n10 | def test_fail():\n   |     ^^^^^^^^^\n   |/ninfo: Test failed here\n  --> test_report.py:12:5\n   |/n12 |     assert False\n   |     ^^^^^^^^^^^^\n   |\n\n"
          },
          "attempts": [
            {
              "attempt": 1,
              "status": "failed",
              "duration_seconds": "[TIME]",
              "captured_output": {
                "stderr": "fail stderr\n"
              },
              "diagnostic": {
                "code": "test-failure",
                "severity": "error",
                "message": "Test `test_fail` failed",
                "rendered": "error[test-failure]: Test `test_fail` failed\n  --> test_report.py:10:5\n   |/n10 | def test_fail():\n   |     ^^^^^^^^^\n   |/ninfo: Test failed here\n  --> test_report.py:12:5\n   |/n12 |     assert False\n   |     ^^^^^^^^^^^^\n   |\n\n"
              }
            },
            {
              "attempt": 2,
              "status": "failed",
              "duration_seconds": "[TIME]",
              "captured_output": {
                "stderr": "fail stderr\n"
              },
              "diagnostic": {
                "code": "test-failure",
                "severity": "error",
                "message": "Test `test_fail` failed",
                "rendered": "error[test-failure]: Test `test_fail` failed\n  --> test_report.py:10:5\n   |/n10 | def test_fail():\n   |     ^^^^^^^^^\n   |/ninfo: Test failed here\n  --> test_report.py:12:5\n   |/n12 |     assert False\n   |     ^^^^^^^^^^^^\n   |\n\n"
              }
            }
          ]
        },
        {
          "module": "test_report",
          "name": "test_flaky",
          "full_name": "test_report::test_flaky",
          "status": "passed",
          "duration_seconds": "[TIME]",
          "flaky": true,
          "retry": {
            "attempts": 2,
            "max_attempts": 2
          },
          "captured_output": {
            "stdout": "flaky attempt 1/nflaky attempt 2\n"
          },
          "attempts": [
            {
              "attempt": 1,
              "status": "failed",
              "duration_seconds": "[TIME]",
              "captured_output": {
                "stdout": "flaky attempt 1\n"
              },
              "diagnostic": {
                "code": "test-failure",
                "severity": "error",
                "message": "Test `test_flaky` failed",
                "rendered": "error[test-failure]: Test `test_flaky` failed\n  --> test_report.py:21:5\n   |/n21 | def test_flaky():\n   |     ^^^^^^^^^^\n   |/ninfo: Test failed here\n  --> test_report.py:26:5\n   |/n26 |     assert count >= 1\n   |     ^^^^^^^^^^^^^^^^^\n   |\n\n"
              }
            },
            {
              "attempt": 2,
              "status": "passed",
              "duration_seconds": "[TIME]",
              "captured_output": {
                "stdout": "flaky attempt 2\n"
              }
            }
          ]
        },
        {
          "module": "test_report",
          "name": "test_pass",
          "full_name": "test_report::test_pass",
          "status": "passed",
          "duration_seconds": "[TIME]",
          "captured_output": {
            "stdout": "pass stdout\n"
          }
        },
        {
          "module": "test_report",
          "name": "test_skip",
          "full_name": "test_report::test_skip",
          "status": "skipped",
          "duration_seconds": "[TIME]",
          "skip_reason": "skip reason"
        }
      ]
    }
    "#);
}

#[test]
fn writes_jsonl_result_records() {
    let context = TestContext::with_file(
        "test_events.py",
        r"
        def test_pass():
            pass

        def test_fail():
            assert False
        ",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--status-level=none",
            "--result-output=reports/events.jsonl",
            "--result-format=jsonl",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_events::test_fail:

    error[test-failure]: Test `test_fail` failed
     --> test_events.py:5:5
      |
    5 | def test_fail():
      |     ^^^^^^^^^
      |
    info: Test failed here
     --> test_events.py:6:5
      |
    6 |     assert False
      |     ^^^^^^^^^^^^
      |

    ────────────
         Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );

    assert_snapshot!(context.read_file("reports/events.jsonl"), @r#"
    {"schema_version":2,"type":"test","module":"test_events","name":"test_fail","full_name":"test_events::test_fail","status":"failed","duration_seconds":"[TIME]","diagnostic":{"code":"test-failure","severity":"error","message":"Test `test_fail` failed","rendered":"error[test-failure]: Test `test_fail` failed\n --> test_events.py:5:5\n  |/n5 | def test_fail():\n  |     ^^^^^^^^^\n  |/ninfo: Test failed here\n --> test_events.py:6:5\n  |/n6 |     assert False\n  |     ^^^^^^^^^^^^\n  |\n\n"}}
    {"schema_version":2,"type":"test","module":"test_events","name":"test_pass","full_name":"test_events::test_pass","status":"passed","duration_seconds":"[TIME]"}
    {"schema_version":2,"type":"run_finished","status":"failed","elapsed_seconds":"[TIME]","stats":{"total":2,"passed":1,"failed":1,"errors":0,"skipped":0,"flaky":0,"slow":0}}
    "#);
}

#[test]
fn fail_slow_reports_teardown_duration_in_json_and_jsonl() {
    let context = TestContext::with_file(
        "test_fail_slow.py",
        r"
import time
import karva

@karva.fixture
def resource():
    yield
    time.sleep(0.4)

@karva.tags.fail_slow(0.2)
def test_resource(resource):
    pass
        ",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--status-level=none",
            "--result-output=reports/results.json",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_fail_slow::test_resource(resource=None):

    error[fail-slow-exceeded]: Test `test_resource` exceeded its fail-slow budget
      --> test_fail_slow.py:11:5
       |
    11 | def test_resource(resource):
       |     ^^^^^^^^^^^^^
       |
    info: Configured budget: [TIME], actual duration: [TIME] (slowest phase: teardown)

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );

    let report = context.read_file("reports/results.json");
    let report_value: Value = serde_json::from_str(&report).expect("parse result report");
    assert!(
        report_value["tests"][0]["duration_seconds"]
            .as_f64()
            .expect("test duration should be numeric")
            >= 0.4
    );
    assert_snapshot!(
        report,
        @r#"
    {
      "schema_version": 2,
      "status": "failed",
      "elapsed_seconds": "[TIME]",
      "stats": {
        "total": 1,
        "passed": 0,
        "failed": 1,
        "errors": 0,
        "skipped": 0,
        "flaky": 0,
        "slow": 0
      },
      "tests": [
        {
          "module": "test_fail_slow",
          "name": "test_resource(resource=None)",
          "full_name": "test_fail_slow::test_resource(resource=None)",
          "status": "failed",
          "duration_seconds": "[TIME]",
          "diagnostic": {
            "code": "fail-slow-exceeded",
            "severity": "error",
            "message": "Test `test_resource` exceeded its fail-slow budget",
            "rendered": "error[fail-slow-exceeded]: Test `test_resource` exceeded its fail-slow budget\n  --> test_fail_slow.py:11:5\n   |/n11 | def test_resource(resource):\n   |     ^^^^^^^^^^^^^\n   |/ninfo: Configured budget: [TIME], actual duration: [TIME] (slowest phase: teardown)\n\n"
          }
        }
      ]
    }
    "#
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--status-level=none",
            "--result-output=reports/results.jsonl",
            "--result-format=jsonl",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_fail_slow::test_resource(resource=None):

    error[fail-slow-exceeded]: Test `test_resource` exceeded its fail-slow budget
      --> test_fail_slow.py:11:5
       |
    11 | def test_resource(resource):
       |     ^^^^^^^^^^^^^
       |
    info: Configured budget: [TIME], actual duration: [TIME] (slowest phase: teardown)

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );

    let report = context.read_file("reports/results.jsonl");
    let test: Value = serde_json::from_str(
        report
            .lines()
            .next()
            .expect("result report should contain a test record"),
    )
    .expect("parse test record");
    assert!(
        test["duration_seconds"]
            .as_f64()
            .expect("test duration should be numeric")
            >= 0.4
    );
    assert_snapshot!(
        report,
        @r#"
    {"schema_version":2,"type":"test","module":"test_fail_slow","name":"test_resource(resource=None)","full_name":"test_fail_slow::test_resource(resource=None)","status":"failed","duration_seconds":"[TIME]","diagnostic":{"code":"fail-slow-exceeded","severity":"error","message":"Test `test_resource` exceeded its fail-slow budget","rendered":"error[fail-slow-exceeded]: Test `test_resource` exceeded its fail-slow budget\n  --> test_fail_slow.py:11:5\n   |/n11 | def test_resource(resource):\n   |     ^^^^^^^^^^^^^\n   |/ninfo: Configured budget: [TIME], actual duration: [TIME] (slowest phase: teardown)\n\n"}}
    {"schema_version":2,"type":"run_finished","status":"failed","elapsed_seconds":"[TIME]","stats":{"total":1,"passed":0,"failed":1,"errors":0,"skipped":0,"flaky":0,"slow":0}}
    "#
    );
}

#[test]
fn writes_related_test_diagnostics() {
    let context = TestContext::with_file(
        "test_related.py",
        r#"
        import karva

        @karva.fixture
        def broken_teardown():
            yield
            raise RuntimeError("teardown failed")

        def test_failure(broken_teardown):
            assert False
        "#,
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--status-level=none",
            "--result-output=reports/related.json",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_related::test_failure(broken_teardown=None):

    error[test-failure]: Test `test_failure` failed
     --> test_related.py:9:5
      |
    9 | def test_failure(broken_teardown):
      |     ^^^^^^^^^^^^
      |
    info: Test ran with arguments:
    info: `broken_teardown`: `None`
    info: Test failed here
      --> test_related.py:10:5
       |
    10 |     assert False
       |     ^^^^^^^^^^^^
       |

    error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `broken_teardown`
     --> test_related.py:5:5
      |
    5 | def broken_teardown():
      |     ^^^^^^^^^^^^^^^
      |
    info: Failed to reset fixture: teardown failed

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "
    );

    assert_snapshot!(context.read_file("reports/related.json"), @r#"
    {
      "schema_version": 2,
      "status": "failed",
      "elapsed_seconds": "[TIME]",
      "stats": {
        "total": 1,
        "passed": 0,
        "failed": 1,
        "errors": 0,
        "skipped": 0,
        "flaky": 0,
        "slow": 0
      },
      "tests": [
        {
          "module": "test_related",
          "name": "test_failure(broken_teardown=None)",
          "full_name": "test_related::test_failure(broken_teardown=None)",
          "status": "failed",
          "duration_seconds": "[TIME]",
          "diagnostic": {
            "code": "test-failure",
            "severity": "error",
            "message": "Test `test_failure` failed",
            "rendered": "error[test-failure]: Test `test_failure` failed\n --> test_related.py:9:5\n  |/n9 | def test_failure(broken_teardown):\n  |     ^^^^^^^^^^^^\n  |/ninfo: Test ran with arguments:/ninfo: `broken_teardown`: `None`/ninfo: Test failed here\n  --> test_related.py:10:5\n   |/n10 |     assert False\n   |     ^^^^^^^^^^^^\n   |\n\n"
          },
          "related_diagnostics": [
            {
              "code": "invalid-fixture-finalizer",
              "severity": "error",
              "message": "Discovered an invalid fixture finalizer `broken_teardown`",
              "rendered": "error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `broken_teardown`\n --> test_related.py:5:5\n  |/n5 | def broken_teardown():\n  |     ^^^^^^^^^^^^^^^\n  |/ninfo: Failed to reset fixture: teardown failed\n\n"
            }
          ]
        }
      ]
    }
    "#);
}

#[test]
fn keeps_run_diagnostics_separate_from_tests() {
    let context = TestContext::with_file(
        "test_import.py",
        r"
        import missing_result_report_dependency

        def test_unreachable():
            pass
        ",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--status-level=none",
            "--result-output=reports/import.json",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test_import`: No module named 'missing_result_report_dependency'

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    "
    );

    assert_snapshot!(context.read_file("reports/import.json"), @r#"
    {
      "schema_version": 2,
      "status": "failed",
      "elapsed_seconds": "[TIME]",
      "stats": {
        "total": 0,
        "passed": 0,
        "failed": 0,
        "errors": 0,
        "skipped": 0,
        "flaky": 0,
        "slow": 0
      },
      "tests": [],
      "run_diagnostics": [
        {
          "code": "failed-to-import-module",
          "severity": "error",
          "message": "Failed to import python module `test_import`: No module named 'missing_result_report_dependency'",
          "rendered": "error[failed-to-import-module]: Failed to import python module `test_import`: No module named 'missing_result_report_dependency'\n\n"
        }
      ]
    }
    "#);
}

#[test]
fn writes_jsonl_run_diagnostic_records() {
    let context = TestContext::with_file(
        "test_import.py",
        r"
        import missing_result_report_dependency

        def test_unreachable():
            pass
        ",
    );

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--status-level=none",
            "--result-output=reports/import.jsonl",
            "--result-format=jsonl",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
    diagnostics:

    error[failed-to-import-module]: Failed to import python module `test_import`: No module named 'missing_result_report_dependency'

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    "
    );

    assert_snapshot!(
        context.read_file("reports/import.jsonl"),
        @r#"
    {"schema_version":2,"type":"run_diagnostic","diagnostic":{"code":"failed-to-import-module","severity":"error","message":"Failed to import python module `test_import`: No module named 'missing_result_report_dependency'","rendered":"error[failed-to-import-module]: Failed to import python module `test_import`: No module named 'missing_result_report_dependency'\n\n"}}
    {"schema_version":2,"type":"run_finished","status":"failed","elapsed_seconds":"[TIME]","stats":{"total":0,"passed":0,"failed":0,"errors":0,"skipped":0,"flaky":0,"slow":0}}
    "#
    );
}

#[test]
fn result_report_status_matches_no_tests_failure() {
    let context = TestContext::new();

    assert_cmd_snapshot!(
        context.command_no_parallel().args([
            "--status-level=none",
            "--result-output=reports/no-tests.json",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped
    error: no tests to run
    (hint: use `--no-tests` to customize)

    ----- stderr -----
    "
    );

    assert_snapshot!(context.read_file("reports/no-tests.json"), @r#"
    {
      "schema_version": 2,
      "status": "failed",
      "elapsed_seconds": "[TIME]",
      "stats": {
        "total": 0,
        "passed": 0,
        "failed": 0,
        "errors": 0,
        "skipped": 0,
        "flaky": 0,
        "slow": 0
      },
      "tests": []
    }
    "#);
}
