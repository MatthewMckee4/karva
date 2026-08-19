//! Recovery when an escaped descendant forces a bounded controller disconnect.

use insta::assert_snapshot;

use super::{run_with_descendant, snapshot_output, terminate_escaped_descendant};
use crate::common::TestContext;

#[test]
fn worker_exit_reports_incomplete_output_from_escaped_descendant() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path
import signal


def test_crash_with_escaped_child():
    ready_read, ready_write = os.pipe()
    if os.fork() == 0:
        os.close(ready_read)
        os.setsid()
        signal.alarm(30)
        Path("descendant_pid").write_text(str(os.getpid()))
        os.write(ready_write, b"1")
        signal.pause()
    os.close(ready_write)
    os.read(ready_read, 1)
    os._exit(30)
"#,
    );
    let output = run_with_descendant(&context);
    terminate_escaped_descendant(&context);

    assert_snapshot!(snapshot_output(&output), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
    diagnostics:

    error[worker-crashed]: Worker 0 terminated with exit code 30 after its 50 ms controller event drain limit expired; the last decoded active checkpoint was `test::test_crash_with_escaped_child`, but later state could not be recovered

    Karva preserved 0 completed test results from this assignment and omitted 1 remaining test selection from recovery because their execution state could not be determined safely. They are absent from the results; rerun them only if repeat execution is safe.

    Worker stderr:
    [Karva stopped draining worker output after the 50 ms limit; final output and results may be incomplete]

    ────────────
         Summary [TIME] 0 tests run: 0 passed, 0 skipped

    ----- stderr -----
    WARN worker output drain limit reached; final output and results may be incomplete worker_id=0 limit_ms=50
    ERROR Worker 0 failed with exit code 30 in [TIME]
    ");
}

#[test]
fn worker_exit_preserves_completed_work_without_retrying_uncertain_selections() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path
import signal
import time


def _complete_once_then_crash_once():
    completed = Path("completed")
    if not completed.exists():
        completed.touch()
        return
    crashed = Path("crashed")
    if not crashed.exists():
        crashed.touch()
        if os.fork() == 0:
            os.setsid()
            signal.alarm(30)
            Path("descendant_pid").write_text(str(os.getpid()))
            signal.pause()
        while not Path("descendant_pid").exists():
            time.sleep(0.001)
        os._exit(35)
    Path("retried_by").write_text(os.environ["KARVA_WORKER_ID"])


def test_a():
    _complete_once_then_crash_once()


def test_b():
    _complete_once_then_crash_once()


def test_c():
    _complete_once_then_crash_once()
"#,
    );
    let output = run_with_descendant(&context);
    terminate_escaped_descendant(&context);
    let output = snapshot_output(&output)
        .replace("`test::test_a`", "`test::test_[CASE]`")
        .replace("`test::test_b`", "`test::test_[CASE]`")
        .replace("`test::test_c`", "`test::test_[CASE]`");

    assert_snapshot!(output, @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 3 tests across 1 worker

    diagnostics:

    error[worker-crashed]: Worker 0 terminated with exit code 35 after its 50 ms controller event drain limit expired; the last decoded active checkpoint was `test::test_[CASE]`, but later state could not be recovered

    Karva preserved 1 completed test result from this assignment and omitted 2 remaining test selections from recovery because their execution state could not be determined safely. They are absent from the results; rerun them only if repeat execution is safe.

    Worker stderr:
    [Karva stopped draining worker output after the 50 ms limit; final output and results may be incomplete]

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    WARN worker output drain limit reached; final output and results may be incomplete worker_id=0 limit_ms=50
    ERROR Worker 0 failed with exit code 35 in [TIME]
    ");
    assert!(context.root().join("completed").exists());
    assert!(!context.root().join("retried_by").exists());
}
