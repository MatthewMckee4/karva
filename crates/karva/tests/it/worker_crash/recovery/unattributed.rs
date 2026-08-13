use insta::assert_snapshot;
use insta_cmd::assert_cmd_snapshot;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
#[cfg(unix)]
use std::path::PathBuf;

use crate::common::TestContext;

#[cfg(unix)]
#[test]
fn worker_exit_before_controller_authentication_is_reported() {
    let context = TestContext::with_file("test.py", "def test_never_started(): pass");
    let worker = context.root().join(".venv/bin/karva-worker");
    std::fs::create_dir_all(worker.parent().expect("worker binary parent"))
        .expect("create project virtualenv bin directory");
    std::fs::write(&worker, "#!/bin/sh\nexit 37\n").expect("write fake worker binary");
    std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o755))
        .expect("make fake worker executable");
    let path = std::env::join_paths(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .filter(|directory| !directory.join("karva-worker").is_file())
            .chain([PathBuf::from("/usr/bin"), PathBuf::from("/bin")]),
    )
    .expect("worker-free executable path");

    assert_cmd_snapshot!(context.command().env("PATH", path), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[worker-crashed]: Worker 0 terminated with exit code 37 during startup before controller authentication

    Karva preserved 0 completed test results from this assignment and is retrying 1 unstarted test selection in a replacement worker.

    error[worker-crashed]: Worker 1 terminated with exit code 37 during startup before controller authentication

    Karva preserved 0 completed test results from this assignment and stopped retrying because the replacement worker committed no new result.

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 37 in [TIME]
    ERROR Worker 1 failed with exit code 37 in [TIME]
    ");
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

    error[worker-crashed]: Worker 0 terminated with exit code 26 with no active test checkpoint

    Karva preserved 0 completed test results from this assignment and is retrying 1 unstarted test selection in a replacement worker.

    error[worker-crashed]: Worker 1 terminated with exit code 26 with no active test checkpoint

    Karva preserved 0 completed test results from this assignment and stopped retrying because the replacement worker committed no new result.

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 26 in [TIME]
    ERROR Worker 1 failed with exit code 26 in [TIME]
    ");
}

#[test]
fn worker_exit_before_first_test_recovers_in_replacement_and_json_report() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path


if os.environ["KARVA_WORKER_ID"] == "0":
    os.write(2, b"stderr during startup\n")
    os._exit(33)


def test_recovered():
    Path("completed-by").write_text(os.environ["KARVA_WORKER_ID"])
"#,
    );

    assert_cmd_snapshot!(
        context.command().arg("--result-output=results.json"),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_recovered

    diagnostics:

    error[worker-crashed]: Worker 0 terminated with exit code 33 with no active test checkpoint

    Karva preserved 0 completed test results from this assignment and is retrying 1 unstarted test selection in a replacement worker.

    Worker stderr:
    stderr during startup

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    stderr during startup
    ERROR Worker 0 failed with exit code 33 in [TIME]
    "
    );
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
          "name": "test_recovered",
          "full_name": "test::test_recovered",
          "status": "passed",
          "duration_seconds": "[TIME]"
        }
      ],
      "run_diagnostics": [
        {
          "code": "worker-crashed",
          "severity": "error",
          "message": "Worker 0 terminated with exit code 33 with no active test checkpoint. Karva preserved 0 completed test results from this assignment and is retrying 1 unstarted test selection in a replacement worker.",
          "rendered": "error[worker-crashed]: Worker 0 terminated with exit code 33 with no active test checkpoint\n/nKarva preserved 0 completed test results from this assignment and is retrying 1 unstarted test selection in a replacement worker.\n/nWorker stderr:/nstderr during startup\n"
        }
      ]
    }
    "#);
    assert_eq!(context.read_file("completed-by"), "1");
}
