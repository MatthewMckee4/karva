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

    assert_cmd_snapshot!(context.command(), @r###"
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
    "###);
    assert_eq!(context.read_file("completed"), "02");
}

#[test]
fn worker_exit_resumes_dynamic_parameter_cases() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path

import karva


@karva.tags.parametrize("value", range(3))
def test_worker_crash(value):
    if value == 1:
        os._exit(19)

    completed = Path("completed")
    completed.write_text(completed.read_text() + str(value) if completed.exists() else str(value))
"#,
    );

    assert_cmd_snapshot!(context.command(), @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_worker_crash(value=0)
           CRASH [TIME] test::test_worker_crash(value=1)
            PASS [TIME] test::test_worker_crash(value=2)

    failures:

    test::test_worker_crash(value=1):

    error[worker-crashed]: Worker terminated with exit code 19 while running `test::test_worker_crash(value=1)`

    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 19 in [TIME]
    "###);
    assert_eq!(context.read_file("completed"), "02");
}

#[test]
fn worker_exit_does_not_repeat_completed_dynamic_parameter_functions() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path

import karva


@karva.tags.parametrize("value", range(2))
def test_a(value):
    completed = Path("completed")
    completed.write_text(completed.read_text() + str(value) if completed.exists() else str(value))


def test_z():
    os._exit(18)
"#,
    );

    assert_cmd_snapshot!(context.command(), @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_a(value=0)
            PASS [TIME] test::test_a(value=1)
           CRASH [TIME] test::test_z

    failures:

    test::test_z:

    error[worker-crashed]: Worker terminated with exit code 18 while running `test::test_z`

    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 18 in [TIME]
    "###);
    assert_eq!(context.read_file("completed"), "01");
}

#[test]
fn worker_exit_recovers_after_multiple_dynamic_case_crashes() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path

import karva


@karva.tags.parametrize("value", range(5))
def test_worker_crash(value):
    if value in (1, 3):
        os._exit(20 + value)

    completed = Path("completed")
    completed.write_text(completed.read_text() + str(value) if completed.exists() else str(value))
"#,
    );

    assert_cmd_snapshot!(context.command(), @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_worker_crash(value=0)
           CRASH [TIME] test::test_worker_crash(value=1)
            PASS [TIME] test::test_worker_crash(value=2)
           CRASH [TIME] test::test_worker_crash(value=3)
            PASS [TIME] test::test_worker_crash(value=4)

    failures:

    test::test_worker_crash(value=1):

    error[worker-crashed]: Worker terminated with exit code 21 while running `test::test_worker_crash(value=1)`

    test::test_worker_crash(value=3):

    error[worker-crashed]: Worker terminated with exit code 23 while running `test::test_worker_crash(value=3)`

    ────────────
         Summary [TIME] 5 tests run: 3 passed, 2 errors, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 21 in [TIME]
    ERROR Worker 1 failed with exit code 23 in [TIME]
    "###);
    assert_eq!(context.read_file("completed"), "024");
}

#[test]
fn worker_exit_obeys_max_fail_before_rescheduling() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path

import karva


@karva.tags.parametrize("value", range(2))
def test_worker_crash(value):
    if value == 0:
        os._exit(25)
    Path("rescheduled").write_text("1")
"#,
    );

    assert_cmd_snapshot!(context.command().arg("--max-fail=1"), @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           CRASH [TIME] test::test_worker_crash(value=0)

    failures:

    test::test_worker_crash(value=0):

    error[worker-crashed]: Worker terminated with exit code 25 while running `test::test_worker_crash(value=0)`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 25 in [TIME]
    "###);
    assert!(!context.root().join("rescheduled").exists());
}

#[test]
fn worker_exit_before_test_start_retries_unstarted_partition_once() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os

os._exit(26)


def test_never_started():
    pass
",
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[worker-crashed]: Worker 0 terminated with exit code 26

    error[worker-crashed]: Worker 1 terminated with exit code 26

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 26 in [TIME]
    ERROR Worker 1 failed with exit code 26 in [TIME]
    ");
}

#[test]
fn completed_test_survives_module_teardown_crash() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os

import karva


@karva.fixture(scope="module")
def crash_during_teardown():
    yield
    os._exit(27)


def test_completed(crash_during_teardown):
    pass
"#,
    );

    assert_cmd_snapshot!(context.command(), @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_completed(crash_during_teardown=None)

    diagnostics:

    error[worker-crashed]: Worker 0 terminated with exit code 27

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 27 in [TIME]
    "###);
}

#[test]
fn no_cache_does_not_require_a_writable_cache_directory() {
    let context = TestContext::with_files([
        (".karva_cache", "not a directory"),
        ("test.py", "def test_pass(): pass"),
    ]);

    assert_cmd_snapshot!(context.command().arg("--no-cache"), @r###"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_pass
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "###);
}

#[cfg(unix)]
#[test]
fn worker_exit_kills_descendants_holding_controller_streams() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os
import signal


def test_crash_with_child():
    if os.fork() == 0:
        signal.pause()
    os._exit(28)
",
    );

    assert_cmd_snapshot!(context.command(), @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           CRASH [TIME] test::test_crash_with_child

    failures:

    test::test_crash_with_child:

    error[worker-crashed]: Worker terminated with exit code 28 while running `test::test_crash_with_child`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 28 in [TIME]
    "###);
}

#[test]
fn worker_stderr_without_newline_is_captured() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os


def test_crash():
    os.write(2, b"stderr without newline")
    os._exit(29)
"#,
    );

    assert_cmd_snapshot!(context.command(), @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           CRASH [TIME] test::test_crash

    failures:

    test::test_crash:

    error[worker-crashed]: Worker terminated with exit code 29 while running `test::test_crash`

    Worker stderr:
    stderr without newline

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    stderr without newline
    ERROR Worker 0 failed with exit code 29 in [TIME]
    "###);
}

#[test]
fn worker_crash_skips_incomplete_coverage_report() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os


def test_crash():
    os._exit(30)
",
    );

    assert_cmd_snapshot!(
        context.command().args(["--cov", "--cov-report=term"]),
        @r###"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           CRASH [TIME] test::test_crash

    failures:

    test::test_crash:

    error[worker-crashed]: Worker terminated with exit code 30 while running `test::test_crash`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 30 in [TIME]
    WARN Coverage report skipped because a crashed worker could not save complete data
    "###
    );
}

#[cfg(unix)]
#[test]
fn worker_abort_is_attributed_to_the_active_test() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os


def test_abort():
    os.abort()
",
    );

    assert_cmd_snapshot!(context.command(), @r###"
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
    "###);
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
            r"
import os
import karva


@karva.fixture
def crash():
    os._exit(0)


def test_crash(crash):
    pass
",
        ),
    ]);

    assert_cmd_snapshot!(
        context.command().args([
            "--status-level=none",
            "--result-output=results.json",
        ]),
        @r###"
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
    "###
    );
    assert_snapshot!(context.read_file("results.json"), @r###"
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
    "###);
    assert_snapshot!(normalize_junit_xml(&context.read_file("junit.xml")), @r###"
    <?xml version="1.0" encoding="UTF-8"?>
    <testsuites name="karva-tests" tests="1" failures="0" skipped="0" errors="1" time="[TIME]">
      <testsuite name="test" tests="1" failures="0" skipped="0" errors="1" time="[TIME]">
        <testcase classname="test" name="test_crash" time="[TIME]">
          <error message="Worker terminated with exit code 0 while running `test::test_crash`" type="worker-crashed">error[worker-crashed]: Worker terminated with exit code 0 while running `test::test_crash`
    </error>
        </testcase>
      </testsuite>
    </testsuites>
    "###);

    let context = TestContext::with_file(
        "test.py",
        r"
import os
import karva


@karva.fixture
def crash():
    os._exit(0)


def test_crash(crash):
    pass
",
    );
    assert_cmd_snapshot!(
        context.command().args([
            "--status-level=none",
            "--result-output=results.jsonl",
            "--result-format=jsonl",
        ]),
        @r###"
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
    "###
    );
    assert_snapshot!(context.read_file("results.jsonl"), @r###"
    {"schema_version":2,"type":"test","module":"test","name":"test_crash","full_name":"test::test_crash","status":"error","duration_seconds":"[TIME]","diagnostic":{"code":"worker-crashed","severity":"error","message":"Worker terminated with exit code 0 while running `test::test_crash`","rendered":"error[worker-crashed]: Worker terminated with exit code 0 while running `test::test_crash`\n"}}
    {"schema_version":2,"type":"run_finished","status":"failed","elapsed_seconds":"[TIME]","stats":{"total":1,"passed":0,"failed":0,"errors":1,"skipped":0,"flaky":0,"slow":0}}
    "###);
}
