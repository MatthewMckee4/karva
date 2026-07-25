use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_unhandled_thread_exceptions_fail_the_active_test() {
    let context = TestContext::with_file(
        "test_thread.py",
        r#"
from threading import Thread


def raise_in_thread(message):
    raise RuntimeError(message)


def test_background_work():
    for index in range(2):
        thread = Thread(
            target=raise_in_thread,
            args=(f"failure {index}",),
            name=f"background-{index}",
        )
        thread.start()
        thread.join()
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_thread::test_background_work

    failures:

    test_thread::test_background_work:

    error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background-0`
     --> test_thread.py:9:5
      |
    9 | def test_background_work():
      |     ^^^^^^^^^^^^^^^^^^^^
      |
    info: Exception raised here
     --> test_thread.py:6:5
      |
    6 |     raise RuntimeError(message)
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: RuntimeError: failure 0

    error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background-1`
     --> test_thread.py:9:5
      |
    9 | def test_background_work():
      |     ^^^^^^^^^^^^^^^^^^^^
      |
    info: Exception raised here
     --> test_thread.py:6:5
      |
    6 |     raise RuntimeError(message)
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: RuntimeError: failure 1

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_unraisable_exception_fails_the_active_test() {
    let context = TestContext::with_file(
        "test_unraisable.py",
        r#"
import gc


class BrokenCleanup:
    def __del__(self):
        raise RuntimeError("cleanup failed")

    def __repr__(self):
        return "BrokenCleanup()"


def test_cleanup():
    value = BrokenCleanup()
    del value
    gc.collect()
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_unraisable::test_cleanup

    failures:

    test_unraisable::test_cleanup:

    error[unraisable-exception]: Unraisable exception `RuntimeError`
      --> test_unraisable.py:13:5
       |
    13 | def test_cleanup():
       |     ^^^^^^^^^^^^
       |
    info: Object: `BrokenCleanup.__del__`
    info: Exception raised here
     --> test_unraisable.py:7:9
      |
    7 |         raise RuntimeError("cleanup failed")
      |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: RuntimeError: cleanup failed

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn test_fixture_lifecycle_exceptions_fail_the_active_test() {
    let context = TestContext::with_file(
        "test_fixture.py",
        r#"
from threading import Thread

import karva


def raise_in_thread(message):
    raise RuntimeError(message)


@karva.fixture
def resource():
    setup_thread = Thread(
        target=raise_in_thread,
        args=("setup failed",),
        name="fixture-setup",
    )
    setup_thread.start()
    setup_thread.join()
    yield "resource"
    teardown_thread = Thread(
        target=raise_in_thread,
        args=("teardown failed",),
        name="fixture-teardown",
    )
    teardown_thread.start()
    teardown_thread.join()


def test_resource(resource):
    assert resource == "resource"
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_fixture::test_resource(resource=resource)

    failures:

    test_fixture::test_resource(resource=resource):

    error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `fixture-setup`
      --> test_fixture.py:30:5
       |
    30 | def test_resource(resource):
       |     ^^^^^^^^^^^^^
       |
    info: Exception raised here
     --> test_fixture.py:8:5
      |
    8 |     raise RuntimeError(message)
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: RuntimeError: setup failed

    error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `fixture-teardown`
      --> test_fixture.py:30:5
       |
    30 | def test_resource(resource):
       |     ^^^^^^^^^^^^^
       |
    info: Exception raised here
     --> test_fixture.py:8:5
      |
    8 |     raise RuntimeError(message)
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: RuntimeError: teardown failed

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_unattributed_exception_fails_the_run() {
    let context = TestContext::with_file(
        "test_import.py",
        r#"
from threading import Thread


def raise_during_import():
    raise RuntimeError("import thread failed")


thread = Thread(target=raise_during_import, name="import-thread")
thread.start()
thread.join()


def test_still_runs():
    pass
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_import::test_still_runs

    diagnostics:

    error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `import-thread`
    info: Exception raised here
     --> test_import.py:6:5
      |
    6 |     raise RuntimeError("import thread failed")
      |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
      |
    info: RuntimeError: import thread failed

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    "#);
}

#[test]
fn test_handled_thread_exception_is_ignored() {
    let context = TestContext::with_file(
        "test_handled.py",
        r#"
from threading import Thread


def handle_exception():
    try:
        raise RuntimeError("handled")
    except RuntimeError:
        pass


def exit_thread():
    raise SystemExit


def test_handled_exception():
    for target in (handle_exception, exit_thread):
        thread = Thread(target=target)
        thread.start()
        thread.join()
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_handled::test_handled_exception
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_unhandled_thread_exception_cannot_be_retried_away() {
    let context = TestContext::with_file(
        "test_retry.py",
        r#"
from threading import Thread


def test_background_work():
    thread = Thread(
        target=lambda: 1 / 0,
        name="retry-thread",
    )
    thread.start()
    thread.join()
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test_retry::test_background_work

    failures:

    test_retry::test_background_work:

    error[unhandled-thread-exception]: Unhandled exception `ZeroDivisionError` in thread `retry-thread`
     --> test_retry.py:5:5
      |
    5 | def test_background_work():
      |     ^^^^^^^^^^^^^^^^^^^^
      |
    info: Exception raised here
     --> test_retry.py:7:9
      |
    7 |         target=lambda: 1 / 0,
      |         ^^^^^^^^^^^^^^^^^^^^^
      |
    info: ZeroDivisionError: division by zero

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_unhandled_thread_exceptions_fail_parallel_workers() {
    let context = TestContext::with_files([
        (
            "test_one.py",
            r#"
from threading import Thread


def test_one():
    thread = Thread(
        target=lambda: 1 / 0,
        name="first-worker-thread",
    )
    thread.start()
    thread.join()
"#,
        ),
        (
            "test_two.py",
            r#"
from threading import Thread


def test_two():
    thread = Thread(
        target=lambda: 1 / 0,
        name="second-worker-thread",
    )
    thread.start()
    thread.join()
"#,
        ),
    ]);

    assert_cmd_snapshot!(
        context
            .command()
            .args(["--num-workers=2", "--status-level=none"]),
        @"
    success: false
    exit_code: 1
    ----- stdout -----

    failures:

    test_one::test_one:

    error[unhandled-thread-exception]: Unhandled exception `ZeroDivisionError` in thread `first-worker-thread`
     --> test_one.py:5:5
      |
    5 | def test_one():
      |     ^^^^^^^^
      |
    info: Exception raised here
     --> test_one.py:7:9
      |
    7 |         target=lambda: 1 / 0,
      |         ^^^^^^^^^^^^^^^^^^^^^
      |
    info: ZeroDivisionError: division by zero

    test_two::test_two:

    error[unhandled-thread-exception]: Unhandled exception `ZeroDivisionError` in thread `second-worker-thread`
     --> test_two.py:5:5
      |
    5 | def test_two():
      |     ^^^^^^^^
      |
    info: Exception raised here
     --> test_two.py:7:9
      |
    7 |         target=lambda: 1 / 0,
      |         ^^^^^^^^^^^^^^^^^^^^^
      |
    info: ZeroDivisionError: division by zero

    ────────────
         Summary [TIME] 2 tests run: 0 passed, 2 failed, 0 skipped

    ----- stderr -----
    "
    );
}
