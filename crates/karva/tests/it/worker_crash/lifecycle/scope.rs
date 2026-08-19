use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

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
    os.write(2, b"stderr during module teardown\n")
    os._exit(27)


def test_completed(crash_during_teardown):
    pass
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_completed(crash_during_teardown=None)

    diagnostics:

    error[worker-crashed]: Worker 0 terminated with exit code 27 with no active test checkpoint

    Karva preserved 1 completed test result from this assignment; no unstarted test selection remained. The worker exited after test execution, during cleanup or shutdown.

    Worker stderr:
    stderr during module teardown

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    stderr during module teardown
    ERROR Worker 0 failed with exit code 27 in [TIME]
    ");
}

#[test]
fn completed_test_survives_next_module_auto_use_setup_crash() {
    let context = TestContext::with_files([
        (
            "test_a.py",
            r#"
from pathlib import Path


def test_completed():
    Path("completed").write_text("a")
"#,
        ),
        (
            "test_b.py",
            r#"
import os
from pathlib import Path

import karva


@karva.fixture(scope="module", auto_use=True)
def crash_during_setup():
    crash_marker = Path("crashed")
    if Path("completed").exists() and not crash_marker.exists():
        crash_marker.write_text("1")
        os._exit(32)


def test_never_started():
    pass
"#,
        ),
    ]);

    assert_cmd_snapshot!(
        context.command().args([
            "--num-workers=1",
            "--shuffle",
            "--random-seed=170938",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
    Random seed: [SEED]
        Starting 2 tests across 1 worker
            PASS [TIME] test_a::test_completed
            PASS [TIME] test_b::test_never_started

    diagnostics:

    error[worker-crashed]: Worker 0 terminated with exit code 32 with no active test checkpoint

    Karva preserved 1 completed test result from this assignment and is retrying 1 unstarted test selection in a replacement worker.

    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 32 in [TIME]
    "
    );
    assert_eq!(context.read_file("completed"), "a");
}
#[test]
fn filtered_replacement_stops_after_an_unattributed_stall() {
    let context = TestContext::with_files([
        (
            "test_a.py",
            r#"
from pathlib import Path


def test_completed():
    Path("completed").write_text("a")
"#,
        ),
        (
            "test_b.py",
            r#"
import os

import karva


@karva.fixture(scope="module", auto_use=True)
def crash_during_setup():
    os._exit(34)


def test_never_started():
    pass
"#,
        ),
    ]);

    assert_cmd_snapshot!(
        context.command().args([
            "--num-workers=1",
            "--shuffle",
            "--random-seed=170938",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
    Random seed: [SEED]
        Starting 2 tests across 1 worker
            PASS [TIME] test_a::test_completed

    diagnostics:

    error[worker-crashed]: Worker 0 terminated with exit code 34 with no active test checkpoint

    Karva preserved 1 completed test result from this assignment and is retrying 1 unstarted test selection in a replacement worker.

    error[worker-crashed]: Worker 1 terminated with exit code 34 with no active test checkpoint

    Karva preserved 0 completed test results from this assignment and stopped retrying because the replacement worker committed no new result.

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 34 in [TIME]
    ERROR Worker 1 failed with exit code 34 in [TIME]
    "
    );
    assert_eq!(context.read_file("completed"), "a");
}

#[test]
fn replacement_progress_allows_one_more_unattributed_retry() {
    let context = TestContext::with_files([
        (
            "test_a.py",
            r#"
import os


def test_crash():
    if os.environ["KARVA_WORKER_ID"] == "0":
        os._exit(40)
"#,
        ),
        (
            "test_b.py",
            r#"
from pathlib import Path


def test_completed():
    Path("completed_b").touch()
"#,
        ),
        (
            "test_c.py",
            r#"
import os
from pathlib import Path

import karva


@karva.fixture(scope="module", auto_use=True)
def crash_replacement_during_setup():
    if os.environ["KARVA_WORKER_ID"] == "1":
        os._exit(41)


def test_recovered():
    Path("completed_c").touch()
"#,
        ),
    ]);

    assert_cmd_snapshot!(
        context.command().args([
            "--num-workers=1",
            "--shuffle",
            "--random-seed=170938",
        ]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----
    Random seed: [SEED]
        Starting 3 tests across 1 worker
           CRASH [TIME] test_a::test_crash
            PASS [TIME] test_b::test_completed
            PASS [TIME] test_c::test_recovered

    failures:

    test_a::test_crash:

    error[worker-crashed]: Worker terminated with exit code 40 while running `test_a::test_crash`

    diagnostics:

    error[worker-crashed]: Worker 1 terminated with exit code 41 with no active test checkpoint

    Karva preserved 1 completed test result from this assignment and is retrying 1 unstarted test selection in a replacement worker.

    ────────────
         Summary [TIME] 3 tests run: 2 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 40 in [TIME]
    ERROR Worker 1 failed with exit code 41 in [TIME]
    "
    );
    assert!(context.root().join("completed_b").is_file());
    assert!(context.root().join("completed_c").is_file());
}
