use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

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
fn worker_exit_recovers_static_cases_across_multiple_workers_exactly_once() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path

import karva


@karva.fixture
def worker_fixture():
    return "fixture"


@karva.tags.parametrize("value", [0, 1, 2, 3, 4, 5, 6, 7, 8, 9])
def test_worker_crash(value, worker_fixture):
    Path("attempt-" + str(value)).touch(exist_ok=False)
    if value == 0:
        os._exit(44)
    Path("completed-" + str(value)).touch()
"#,
    );

    assert_cmd_snapshot!(
        context
            .command()
            .args(["--num-workers=2", "--status-level=none"]),
        @r###"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test::test_worker_crash(value=0, worker_fixture='fixture'):

    error[worker-crashed]: Worker terminated with exit code 44 while running `test::test_worker_crash(value=0, worker_fixture='fixture')`

    ────────────
         Summary [TIME] 10 tests run: 9 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 44 in [TIME]
    "###
    );
    for value in 0..10 {
        assert!(context.root().join(format!("attempt-{value}")).is_file());
        assert_eq!(
            context.root().join(format!("completed-{value}")).is_file(),
            value != 0
        );
    }
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
fn worker_parameter_display_crash_is_attributed_to_active_test() {
    // The decorator renders once during import. The second render must happen
    // only after the worker flushes its unresolved lifecycle checkpoint.
    let context = TestContext::with_file(
        "test.py",
        r#"
import os

import karva


class Value(int):
    calls = 0

    def __str__(self):
        type(self).calls += 1
        if type(self).calls == 2:
            os._exit(31)
        return "value"


@karva.tags.parametrize("value", [Value(1)])
def test_parameter_display_crash(value):
    pass
"#,
    );

    assert_cmd_snapshot!(context.command(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
           CRASH [TIME] test::test_parameter_display_crash

    failures:

    test::test_parameter_display_crash:

    error[worker-crashed]: Worker terminated with exit code 31 while running `test::test_parameter_display_crash`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

    ----- stderr -----
    ERROR Worker 0 failed with exit code 31 in [TIME]
    ");
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
