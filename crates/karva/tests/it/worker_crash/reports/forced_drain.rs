//! Machine-report behavior when controller event draining must be interrupted.

use std::process::Command;

use insta::assert_snapshot;
use insta_cmd::assert_cmd_snapshot;

use super::normalize_junit_xml;
use crate::common::TestContext;

fn forced_drain_context() -> TestContext {
    TestContext::with_files([
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
from pathlib import Path
import signal


def test_a_pass():
    pass


def test_b_crash():
    ready_read, ready_write = os.pipe()
    if os.fork() == 0:
        os.close(ready_read)
        os.setsid()
        # `--status-level=none` inherits worker stdout; do not retain the test
        # harness capture pipes while deliberately retaining controller IPC.
        os.close(1)
        os.close(2)
        # Cleanup fallback: 30 seconds is 600 event-drain windows.
        signal.alarm(30)
        Path("descendant_pid").write_text(str(os.getpid()))
        os.write(ready_write, b"1")
        signal.pause()
    os.close(ready_write)
    os.read(ready_read, 1)
    os._exit(35)
"#,
        ),
    ])
}

#[test]
fn forced_drain_is_reported_consistently_across_machine_reports() {
    let context = forced_drain_context();

    assert_cmd_snapshot!(
        context.command().args([
            "--status-level=none",
            "--num-workers=1",
            "--result-output=results.json",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    diagnostics:

    error[worker-crashed]: Worker 0 terminated with exit code 35 after its 50 ms controller event drain limit expired; the last decoded active checkpoint was `test::test_b_crash`, but later state could not be recovered

    Karva preserved 1 completed test result from this assignment and omitted 1 remaining test selection from recovery because their execution state could not be determined safely. They are absent from the results; rerun them only if repeat execution is safe.

    Worker stderr:
    [Karva stopped draining worker output after the 50 ms limit; final output and results may be incomplete]

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    WARN worker output drain limit reached; final output and results may be incomplete worker_id=0 limit_ms=50
    ERROR Worker 0 failed with exit code 35 in [TIME]
    "
    );
    terminate_escaped_descendant(&context);
    assert_snapshot!(context.read_file("results.json"), @r#"
    {
      "schema_version": 2,
      "status": "failed",
      "elapsed_seconds": "[TIME]",
      "stats": {
        "total": 1,
        "passed": 1,
        "failed": 0,
        "errors": 0,
        "skipped": 0,
        "flaky": 0,
        "slow": 0
      },
      "tests": [
        {
          "module": "test",
          "name": "test_a_pass",
          "full_name": "test::test_a_pass",
          "status": "passed",
          "duration_seconds": "[TIME]"
        }
      ],
      "run_diagnostics": [
        {
          "code": "worker-crashed",
          "severity": "error",
          "message": "Worker 0 terminated with exit code 35 after its 50 ms controller event drain limit expired; the last decoded active checkpoint was `test::test_b_crash`, but later state could not be recovered. Karva preserved 1 completed test result from this assignment and omitted 1 remaining test selection from recovery because their execution state could not be determined safely. They are absent from the results; rerun them only if repeat execution is safe.",
          "rendered": "error[worker-crashed]: Worker 0 terminated with exit code 35 after its 50 ms controller event drain limit expired; the last decoded active checkpoint was `test::test_b_crash`, but later state could not be recovered\n/nKarva preserved 1 completed test result from this assignment and omitted 1 remaining test selection from recovery because their execution state could not be determined safely. They are absent from the results; rerun them only if repeat execution is safe.\n/nWorker stderr:\n[Karva stopped draining worker output after the 50 ms limit; final output and results may be incomplete]\n"
        }
      ]
    }
    "#);
    assert_snapshot!(normalize_junit_xml(&context.read_file("junit.xml")), @r#"
    <?xml version="1.0" encoding="UTF-8"?>
    <testsuites name="karva-tests" tests="2" failures="0" skipped="0" errors="1" time="[TIME]">
      <testsuite name="test" tests="1" failures="0" skipped="0" errors="0" time="[TIME]">
        <testcase classname="test" name="test_a_pass" time="[TIME]"/>
      </testsuite>
      <testsuite name="karva-tests::run" tests="1" failures="0" skipped="0" errors="1" time="[TIME]">
        <testcase classname="karva.run" name="worker-crashed-1" time="[TIME]">
          <error message="Worker 0 terminated with exit code 35 after its 50 ms controller event drain limit expired; the last decoded active checkpoint was `test::test_b_crash`, but later state could not be recovered. Karva preserved 1 completed test result from this assignment and omitted 1 remaining test selection from recovery because their execution state could not be determined safely. They are absent from the results; rerun them only if repeat execution is safe." type="worker-crashed">error[worker-crashed]: Worker 0 terminated with exit code 35 after its 50 ms controller event drain limit expired; the last decoded active checkpoint was `test::test_b_crash`, but later state could not be recovered

    Karva preserved 1 completed test result from this assignment and omitted 1 remaining test selection from recovery because their execution state could not be determined safely. They are absent from the results; rerun them only if repeat execution is safe.

    Worker stderr:
    [Karva stopped draining worker output after the 50 ms limit; final output and results may be incomplete]
    </error>
        </testcase>
      </testsuite>
    </testsuites>
    "#);

    let context = forced_drain_context();
    assert_cmd_snapshot!(
        context.command().args([
            "--status-level=none",
            "--num-workers=1",
            "--result-output=results.jsonl",
            "--result-format=jsonl",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    diagnostics:

    error[worker-crashed]: Worker 0 terminated with exit code 35 after its 50 ms controller event drain limit expired; the last decoded active checkpoint was `test::test_b_crash`, but later state could not be recovered

    Karva preserved 1 completed test result from this assignment and omitted 1 remaining test selection from recovery because their execution state could not be determined safely. They are absent from the results; rerun them only if repeat execution is safe.

    Worker stderr:
    [Karva stopped draining worker output after the 50 ms limit; final output and results may be incomplete]

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    WARN worker output drain limit reached; final output and results may be incomplete worker_id=0 limit_ms=50
    ERROR Worker 0 failed with exit code 35 in [TIME]
    "
    );
    terminate_escaped_descendant(&context);
    assert_snapshot!(context.read_file("results.jsonl"), @r#"
    {"schema_version":2,"type":"test","module":"test","name":"test_a_pass","full_name":"test::test_a_pass","status":"passed","duration_seconds":"[TIME]"}
    {"schema_version":2,"type":"run_diagnostic","diagnostic":{"code":"worker-crashed","severity":"error","message":"Worker 0 terminated with exit code 35 after its 50 ms controller event drain limit expired; the last decoded active checkpoint was `test::test_b_crash`, but later state could not be recovered. Karva preserved 1 completed test result from this assignment and omitted 1 remaining test selection from recovery because their execution state could not be determined safely. They are absent from the results; rerun them only if repeat execution is safe.","rendered":"error[worker-crashed]: Worker 0 terminated with exit code 35 after its 50 ms controller event drain limit expired; the last decoded active checkpoint was `test::test_b_crash`, but later state could not be recovered\n/nKarva preserved 1 completed test result from this assignment and omitted 1 remaining test selection from recovery because their execution state could not be determined safely. They are absent from the results; rerun them only if repeat execution is safe.\n/nWorker stderr:\n[Karva stopped draining worker output after the 50 ms limit; final output and results may be incomplete]\n"}}
    {"schema_version":2,"type":"run_finished","status":"failed","elapsed_seconds":"[TIME]","stats":{"total":1,"passed":1,"failed":0,"errors":0,"skipped":0,"flaky":0,"slow":0}}
    "#);
}

fn terminate_escaped_descendant(context: &TestContext) {
    let pid = context.read_file("descendant_pid");
    let status = Command::new("kill")
        .args(["-KILL", pid.trim()])
        .status()
        .expect("kill escaped worker descendant");
    assert!(status.success(), "failed to kill worker descendant {pid}");
}
