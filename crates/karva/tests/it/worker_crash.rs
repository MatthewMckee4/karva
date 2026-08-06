use insta::assert_snapshot;
use insta_cmd::assert_cmd_snapshot;
use regex::Regex;

use crate::common::TestContext;

fn normalize_junit_xml(xml: &str) -> String {
    Regex::new(r#"time="[0-9.]+""#)
        .expect("valid time regex")
        .replace_all(xml, r#"time="[TIME]""#)
        .to_string()
}

#[test]
fn worker_exit_preserves_results_and_reschedules_unstarted_tests() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path

import karva


@karva.tags.parametrize("value", [0, 1, 2])
def test_worker_crash(value):
    if value == 1:
        os.write(2, b"stderr before crash\n")
        os._exit(17)

    completed = Path("completed")
    completed.write_text(completed.read_text() + str(value) if completed.exists() else str(value))
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_worker_crash(value=0)
           CRASH [TIME] test::test_worker_crash(value=1)
            PASS [TIME] test::test_worker_crash(value=2)

    failures:

    test::test_worker_crash(value=1):

    error[worker-crashed]: Worker terminated with exit code 17 while running `test::test_worker_crash(value=1)`

    Worker stderr:
    stderr before crash

    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 error, 0 skipped

    ----- stderr -----
    stderr before crash
    ERROR Worker 0 failed with exit code 17 in [TIME]
    ");
    assert_eq!(context.read_file("completed"), "02");
}

#[cfg(unix)]
#[test]
fn worker_abort_is_attributed_to_the_active_test() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os


def test_abort():
    os.abort()
"#,
    );

    assert_cmd_snapshot!(context.command(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           CRASH [TIME] test::test_abort

    failures:

    test::test_abort:

    error[worker-crashed]: Worker terminated with SIGABRT (6) while running `test::test_abort`

    Worker stderr:
    Fatal Python error: Aborted

    Current thread [THREAD] (most recent call first):
      File "<temp_dir>/test.py", line 6 in test_abort
      File "<venv>/bin/karva-worker", line 10 in <module>

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    Fatal Python error: Aborted

    Current thread [THREAD] (most recent call first):
      File "<temp_dir>/test.py", line 6 in test_abort
      File "<venv>/bin/karva-worker", line 10 in <module>
    ERROR Worker 0 failed with SIGABRT (6) in [TIME]
    "#);
}

#[test]
fn worker_crash_is_consistent_across_machine_reports() {
    let context = TestContext::with_files([
        (
            "karva.toml",
            r#"
[profile.default.junit]
path = "junit.xml"
"#,
        ),
        (
            "test.py",
            r#"
import os
import karva


@karva.fixture
def crash():
    os._exit(0)


def test_crash(crash):
    pass
"#,
        ),
    ]);

    assert_cmd_snapshot!(
        context.command().args([
            "--status-level=none",
            "--result-output=results.json",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test::test_crash:

    error[worker-crashed]: Worker terminated with exit code 0 while running `test::test_crash`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 0 in [TIME]
    "
    );
    assert_snapshot!(context.read_file("results.json"), @r#"
    {
      "schema_version": 2,
      "status": "failed",
      "elapsed_seconds": "[TIME]",
      "stats": {
        "total": 1,
        "passed": 0,
        "failed": 0,
        "errors": 1,
        "skipped": 0,
        "flaky": 0,
        "slow": 0
      },
      "tests": [
        {
          "module": "test",
          "name": "test_crash",
          "full_name": "test::test_crash",
          "status": "error",
          "duration_seconds": "[TIME]",
          "diagnostic": {
            "code": "worker-crashed",
            "severity": "error",
            "message": "Worker terminated with exit code 0 while running `test::test_crash`",
            "rendered": "error[worker-crashed]: Worker terminated with exit code 0 while running `test::test_crash`\n"
          }
        }
      ]
    }
    "#);
    assert_snapshot!(normalize_junit_xml(&context.read_file("junit.xml")), @r#"
    <?xml version="1.0" encoding="UTF-8"?>
    <testsuites name="karva-tests" tests="1" failures="0" skipped="0" errors="1" time="[TIME]">
      <testsuite name="test" tests="1" failures="0" skipped="0" errors="1" time="[TIME]">
        <testcase classname="test" name="test_crash" time="[TIME]">
          <error message="Worker terminated with exit code 0 while running `test::test_crash`" type="worker-crashed">error[worker-crashed]: Worker terminated with exit code 0 while running `test::test_crash`
    </error>
        </testcase>
      </testsuite>
    </testsuites>
    "#);

    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import karva


@karva.fixture
def crash():
    os._exit(0)


def test_crash(crash):
    pass
"#,
    );
    assert_cmd_snapshot!(
        context.command().args([
            "--status-level=none",
            "--result-output=results.jsonl",
            "--result-format=jsonl",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test::test_crash:

    error[worker-crashed]: Worker terminated with exit code 0 while running `test::test_crash`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 0 in [TIME]
    "
    );
    assert_snapshot!(context.read_file("results.jsonl"), @r#"
    {"schema_version":2,"type":"test","module":"test","name":"test_crash","full_name":"test::test_crash","status":"error","duration_seconds":"[TIME]","diagnostic":{"code":"worker-crashed","severity":"error","message":"Worker terminated with exit code 0 while running `test::test_crash`","rendered":"error[worker-crashed]: Worker terminated with exit code 0 while running `test::test_crash`\n"}}
    {"schema_version":2,"type":"run_finished","status":"failed","elapsed_seconds":"[TIME]","stats":{"total":1,"passed":0,"failed":0,"errors":1,"skipped":0,"flaky":0,"slow":0}}
    "#);
}
