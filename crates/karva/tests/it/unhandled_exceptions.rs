//! Exceptions escaping Python calls must survive lifecycle and report boundaries.

use insta::internals::SettingsBindDropGuard;
use insta::{Settings, assert_snapshot};
use insta_cmd::assert_cmd_snapshot;
use karva_static::ToolEnvVars;
use rstest::rstest;

use crate::common::TestContext;

/// Python 3.14 moved unraisable object details into `err_msg` and clears `object`.
fn modern_unraisable() -> bool {
    let version = std::env::var(ToolEnvVars::PYTHON_VERSION).unwrap_or_else(|_| "3.15".to_string());
    version
        .split('.')
        .nth(1)
        .and_then(|minor| minor.parse::<u32>().ok())
        .is_some_and(|minor| minor >= 14)
}

/// Normalizes function addresses and timings while retaining every diagnostic.
fn background_snapshots() -> SettingsBindDropGuard {
    let mut settings = Settings::clone_current();
    settings.add_filter(r"(function [^<>]+ at )0x[0-9a-fA-F]+", "$1[ADDRESS]");
    settings.add_filter(r#"time="[0-9.]+""#, "time=\"[TIME]\"");
    settings.bind_to_scope()
}

const HELPERS: &str = r#"
import threading
import karva


def fail():
    raise RuntimeError("background failed")


class BrokenCleanup:
    def __del__(self):
        raise RuntimeError("cleanup failed")


def emit(kind):
    if kind == "thread":
        thread = threading.Thread(target=fail, name="background")
        thread.start()
        thread.join()
    else:
        value = BrokenCleanup()
        del value
"#;

#[rstest]
fn errors_belong_to_the_active_attempt(
    #[values("thread", "unraisable")] kind: &str,
    #[values("setup", "call", "teardown")] phase: &str,
) {
    let context = TestContext::with_file(
        "test_background.py",
        format!(
            r#"{HELPERS}
@karva.fixture
def resource():
    if "{phase}" == "setup":
        emit("{kind}")
    yield
    if "{phase}" == "teardown":
        emit("{kind}")


def test_background(resource):
    if "{phase}" == "call":
        emit("{kind}")
"#
        ),
    );
    let _snapshots = background_snapshots();
    if kind == "thread" {
        assert_cmd_snapshot!(context.command_no_parallel(), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test_background::test_background(resource=None)

        failures:

        test_background::test_background(resource=None):

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_background.py:33:5
           |
        33 | def test_background(resource):
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --> test_background.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
    } else if modern_unraisable() {
        assert_cmd_snapshot!(context.command_no_parallel(), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test_background::test_background(resource=None)

        failures:

        test_background::test_background(resource=None):

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:33:5
           |
        33 | def test_background(resource):
           |     ^^^^^^^^^^^^^^^
        info: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>
        info: Object: `None`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
    } else {
        assert_cmd_snapshot!(context.command_no_parallel(), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test_background::test_background(resource=None)

        failures:

        test_background::test_background(resource=None):

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:33:5
           |
        33 | def test_background(resource):
           |     ^^^^^^^^^^^^^^^
        info: Object: `BrokenCleanup.__del__`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
    }
}

#[test]
fn retains_multiple_events_and_existing_failure() {
    let context = TestContext::with_file(
        "test_background.py",
        format!(
            r#"{HELPERS}
def test_background():
    emit("thread")
    emit("unraisable")
    emit("thread")
    emit("unraisable")
    raise ValueError("test body failed too")
"#
        ),
    );
    let _snapshots = background_snapshots();
    if modern_unraisable() {
        assert_cmd_snapshot!(context.command_no_parallel(), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
                FAIL [TIME] test_background::test_background

        failures:

        test_background::test_background:

        error[test-failure]: Test `test_background` failed
          --> test_background.py:24:5
           |
        24 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Test failed here
          --> test_background.py:29:5
           |
        29 |     raise ValueError("test body failed too")
           |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: test body failed too

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_background.py:24:5
           |
        24 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --> test_background.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:24:5
           |
        24 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>
        info: Object: `None`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_background.py:24:5
           |
        24 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --> test_background.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:24:5
           |
        24 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>
        info: Object: `None`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

        ----- stderr -----
        "#);
    } else {
        assert_cmd_snapshot!(context.command_no_parallel(), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
                FAIL [TIME] test_background::test_background

        failures:

        test_background::test_background:

        error[test-failure]: Test `test_background` failed
          --> test_background.py:24:5
           |
        24 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Test failed here
          --> test_background.py:29:5
           |
        29 |     raise ValueError("test body failed too")
           |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: test body failed too

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_background.py:24:5
           |
        24 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --> test_background.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:24:5
           |
        24 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Object: `BrokenCleanup.__del__`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_background.py:24:5
           |
        24 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --> test_background.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:24:5
           |
        24 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Object: `BrokenCleanup.__del__`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

        ----- stderr -----
        "#);
    }
}

#[rstest]
fn unattributed_events_fail_the_run(
    #[values("thread", "unraisable")] kind: &str,
    #[values("import", "session_setup", "session_teardown", "module_teardown")] phase: &str,
) {
    let context = TestContext::with_file("test_background.py", "def test_passes(): pass");
    let scope = if phase == "module_teardown" {
        "module"
    } else {
        "session"
    };
    let provider = if phase == "module_teardown" {
        "test_background.py"
    } else {
        "conftest.py"
    };
    context.write_file(
        provider,
        format!(
            r#"{HELPERS}
if "{phase}" == "import":
    emit("{kind}")


@karva.fixture(scope="{scope}", auto_use=True)
def resource():
    if "{phase}" == "session_setup":
        emit("{kind}")
    yield
    if "{phase}" in ("session_teardown", "module_teardown"):
        emit("{kind}")


def test_passes():
    pass
"#
        ),
    );
    let _snapshots = background_snapshots();
    if phase == "module_teardown" {
        if kind == "thread" {
            assert_cmd_snapshot!(context.command_no_parallel(), @r#"
            success: false
            exit_code: 1
            ----- stdout -----
                Starting 1 test across 1 worker
                    PASS [TIME] test_background::test_passes

            diagnostics:

            error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
            info: Exception raised here
             --> test_background.py:7:5
              |
            7 |     raise RuntimeError("background failed")
              |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
            info: RuntimeError: background failed

            ────────────
                 Summary [TIME] 1 test run: 1 passed, 0 skipped

            ----- stderr -----
            "#);
        } else if modern_unraisable() {
            assert_cmd_snapshot!(context.command_no_parallel(), @r#"
            success: false
            exit_code: 1
            ----- stdout -----
                Starting 1 test across 1 worker
                    PASS [TIME] test_background::test_passes

            diagnostics:

            error[unraisable-exception]: Unraisable exception `RuntimeError`
            info: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>
            info: Object: `None`
            info: Exception raised here
              --> test_background.py:12:9
               |
            12 |         raise RuntimeError("cleanup failed")
               |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
            info: RuntimeError: cleanup failed

            ────────────
                 Summary [TIME] 1 test run: 1 passed, 0 skipped

            ----- stderr -----
            "#);
        } else {
            assert_cmd_snapshot!(context.command_no_parallel(), @r#"
            success: false
            exit_code: 1
            ----- stdout -----
                Starting 1 test across 1 worker
                    PASS [TIME] test_background::test_passes

            diagnostics:

            error[unraisable-exception]: Unraisable exception `RuntimeError`
            info: Object: `BrokenCleanup.__del__`
            info: Exception raised here
              --> test_background.py:12:9
               |
            12 |         raise RuntimeError("cleanup failed")
               |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
            info: RuntimeError: cleanup failed

            ────────────
                 Summary [TIME] 1 test run: 1 passed, 0 skipped

            ----- stderr -----
            "#);
        }
    } else {
        if kind == "thread" {
            assert_cmd_snapshot!(context.command_no_parallel(), @r#"
            success: false
            exit_code: 1
            ----- stdout -----
                Starting 1 test across 1 worker
                    PASS [TIME] test_background::test_passes

            diagnostics:

            error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
            info: Exception raised here
             --> conftest.py:7:5
              |
            7 |     raise RuntimeError("background failed")
              |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
            info: RuntimeError: background failed

            ────────────
                 Summary [TIME] 1 test run: 1 passed, 0 skipped

            ----- stderr -----
            "#);
        } else if modern_unraisable() {
            assert_cmd_snapshot!(context.command_no_parallel(), @r#"
            success: false
            exit_code: 1
            ----- stdout -----
                Starting 1 test across 1 worker
                    PASS [TIME] test_background::test_passes

            diagnostics:

            error[unraisable-exception]: Unraisable exception `RuntimeError`
            info: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>
            info: Object: `None`
            info: Exception raised here
              --> conftest.py:12:9
               |
            12 |         raise RuntimeError("cleanup failed")
               |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
            info: RuntimeError: cleanup failed

            ────────────
                 Summary [TIME] 1 test run: 1 passed, 0 skipped

            ----- stderr -----
            "#);
        } else {
            assert_cmd_snapshot!(context.command_no_parallel(), @r#"
            success: false
            exit_code: 1
            ----- stdout -----
                Starting 1 test across 1 worker
                    PASS [TIME] test_background::test_passes

            diagnostics:

            error[unraisable-exception]: Unraisable exception `RuntimeError`
            info: Object: `BrokenCleanup.__del__`
            info: Exception raised here
              --> conftest.py:12:9
               |
            12 |         raise RuntimeError("cleanup failed")
               |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
            info: RuntimeError: cleanup failed

            ────────────
                 Summary [TIME] 1 test run: 1 passed, 0 skipped

            ----- stderr -----
            "#);
        }
    }
}

#[test]
fn handled_exceptions_and_thread_system_exit_still_pass() {
    let context = TestContext::with_file(
        "test_background.py",
        r#"
from concurrent.futures import ThreadPoolExecutor
from threading import Thread


def handled():
    try:
        raise ValueError("handled")
    except ValueError:
        pass


def exits():
    raise SystemExit


def fails():
    raise ValueError("stored on future")


def test_handled():
    for target in (handled, exits):
        thread = Thread(target=target)
        thread.start()
        thread.join()
    with ThreadPoolExecutor() as executor:
        future = executor.submit(fails)
        try:
            future.result()
        except ValueError:
            pass
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_background::test_handled
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn retries_cannot_hide_background_errors(#[values("thread", "unraisable")] kind: &str) {
    let context = TestContext::with_file(
        "test_background.py",
        format!(
            r#"{HELPERS}
import os


def test_background():
    if os.environ["KARVA_ATTEMPT"] == "1":
        emit("{kind}")
"#
        ),
    );
    let _snapshots = background_snapshots();
    if kind == "thread" {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test_background::test_background

        failures:

        test_background::test_background:

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_background.py:27:5
           |
        27 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --> test_background.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
    } else if modern_unraisable() {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test_background::test_background

        failures:

        test_background::test_background:

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:27:5
           |
        27 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>
        info: Object: `None`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
    } else {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test_background::test_background

        failures:

        test_background::test_background:

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:27:5
           |
        27 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Object: `BrokenCleanup.__del__`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
    }
}

#[test]
fn events_do_not_leak_between_parameter_cases_or_parallel_workers() {
    let context = TestContext::with_files([
        (
            "test_one.py",
            &*format!(
                r#"{HELPERS}
@karva.tags.parametrize("kind", ["thread", "unraisable", "handled"])
def test_background(kind):
    if kind != "handled":
        emit(kind)
"#
            ),
        ),
        (
            "test_two.py",
            &*format!(
                r#"{HELPERS}
def test_background():
    emit("thread")


def test_passes():
    pass
"#
            ),
        ),
    ]);
    let _snapshots = background_snapshots();
    if modern_unraisable() {
        assert_cmd_snapshot!(context
            .command()
            .args(["--num-workers=2", "--status-level=none"]), @r#"
        success: false
        exit_code: 1
        ----- stdout -----

        failures:

        test_one::test_background(kind='thread'):

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_one.py:25:5
           |
        25 | def test_background(kind):
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --> test_one.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        test_one::test_background(kind='unraisable'):

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_one.py:25:5
           |
        25 | def test_background(kind):
           |     ^^^^^^^^^^^^^^^
        info: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>
        info: Object: `None`
        info: Exception raised here
          --> test_one.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        test_two::test_background:

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_two.py:24:5
           |
        24 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --> test_two.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        ────────────
             Summary [TIME] 5 tests run: 2 passed, 3 errors, 0 skipped

        ----- stderr -----
        "#);
    } else {
        assert_cmd_snapshot!(context
            .command()
            .args(["--num-workers=2", "--status-level=none"]), @r#"
        success: false
        exit_code: 1
        ----- stdout -----

        failures:

        test_one::test_background(kind='thread'):

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_one.py:25:5
           |
        25 | def test_background(kind):
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --> test_one.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        test_one::test_background(kind='unraisable'):

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_one.py:25:5
           |
        25 | def test_background(kind):
           |     ^^^^^^^^^^^^^^^
        info: Object: `BrokenCleanup.__del__`
        info: Exception raised here
          --> test_one.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        test_two::test_background:

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_two.py:24:5
           |
        24 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --> test_two.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        ────────────
             Summary [TIME] 5 tests run: 2 passed, 3 errors, 0 skipped

        ----- stderr -----
        "#);
    }
}

#[test]
fn background_errors_override_skip_and_expected_failure() {
    let context = TestContext::with_file(
        "test_background.py",
        format!(
            r#"{HELPERS}
def test_skips():
    emit("thread")
    karva.skip("skip must not hide the error")


@karva.tags.expect_fail
def test_expected_failure():
    emit("unraisable")
    raise ValueError("expected")
"#
        ),
    );
    let _snapshots = background_snapshots();
    if modern_unraisable() {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @r#"
        success: false
        exit_code: 1
        ----- stdout -----

        failures:

        test_background::test_expected_failure:

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:30:5
           |
        30 | def test_expected_failure():
           |     ^^^^^^^^^^^^^^^^^^^^^
        info: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>
        info: Object: `None`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        test_background::test_skips:

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_background.py:24:5
           |
        24 | def test_skips():
           |     ^^^^^^^^^^
        info: Exception raised here
         --> test_background.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        ────────────
             Summary [TIME] 2 tests run: 0 passed, 2 errors, 0 skipped

        ----- stderr -----
        "#);
    } else {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @r#"
        success: false
        exit_code: 1
        ----- stdout -----

        failures:

        test_background::test_expected_failure:

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:30:5
           |
        30 | def test_expected_failure():
           |     ^^^^^^^^^^^^^^^^^^^^^
        info: Object: `BrokenCleanup.__del__`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        test_background::test_skips:

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_background.py:24:5
           |
        24 | def test_skips():
           |     ^^^^^^^^^^
        info: Exception raised here
         --> test_background.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        ────────────
             Summary [TIME] 2 tests run: 0 passed, 2 errors, 0 skipped

        ----- stderr -----
        "#);
    }
}

#[test]
fn object_and_exception_formatting_cannot_hide_errors() {
    let context = TestContext::with_file(
        "test_background.py",
        r#"
import threading
import weakref


class BrokenMessage(Exception):
    def __setattr__(self, name, value):
        if name == "__traceback__":
            raise ValueError("traceback setter must not run")
        super().__setattr__(name, value)

    def __str__(self):
        raise ValueError("str failed")


class Callback:
    def __call__(self, reference):
        raise BrokenMessage()

    def __repr__(self):
        raise RuntimeError("repr must not run")


class Value:
    pass


def test_callback():
    value = Value()
    reference = weakref.ref(value, Callback())
    del value
    assert reference() is None
"#,
    );
    let _snapshots = background_snapshots();
    if modern_unraisable() {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test_background::test_callback

        failures:

        test_background::test_callback:

        error[unraisable-exception]: Unraisable exception `BrokenMessage`
          --> test_background.py:28:5
           |
        28 | def test_callback():
           |     ^^^^^^^^^^^^^
        info: Object: `None`
        info: Exception raised here
          --> test_background.py:18:9
           |
        18 |         raise BrokenMessage()
           |         ^^^^^^^^^^^^^^^^^^^^^
        info: BrokenMessage: <exception message unavailable>

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        ");
    } else {
        assert_cmd_snapshot!(context.command_no_parallel(), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test_background::test_callback

        failures:

        test_background::test_callback:

        error[unraisable-exception]: Unraisable exception `BrokenMessage`
          --> test_background.py:28:5
           |
        28 | def test_callback():
           |     ^^^^^^^^^^^^^
        info: Object: `<Callback object>`
        info: Exception raised here
          --> test_background.py:18:9
           |
        18 |         raise BrokenMessage()
           |         ^^^^^^^^^^^^^^^^^^^^^
        info: BrokenMessage: <exception message unavailable>

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        ");
    }
}

#[rstest]
fn machine_reports_retain_test_and_worker_events(#[values("json", "jsonl")] format: &str) {
    let context = TestContext::with_file(
        "test_background.py",
        format!(
            r#"{HELPERS}
emit("thread")
emit("unraisable")


def test_background():
    emit("thread")
    emit("unraisable")
"#
        ),
    );
    context.write_file(
        "karva.toml",
        "[profile.default.junit]\npath = \"results.xml\"\n",
    );
    let _snapshots = background_snapshots();
    let mut command = context.command_no_parallel();
    command.args([
        "--status-level=none",
        "--result-output=results.out",
        &format!("--result-format={format}"),
    ]);
    if modern_unraisable() {
        assert_cmd_snapshot!(command, @r#"
        success: false
        exit_code: 1
        ----- stdout -----

        failures:

        test_background::test_background:

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_background.py:28:5
           |
        28 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --> test_background.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:28:5
           |
        28 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>
        info: Object: `None`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        diagnostics:

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
        info: Exception raised here
         --> test_background.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        error[unraisable-exception]: Unraisable exception `RuntimeError`
        info: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>
        info: Object: `None`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
        if format == "json" {
            assert_snapshot!(context.read_file("results.out"), @r#"
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
                "expected_failure": 0,
                "flaky": 0,
                "slow": 0
              },
              "tests": [
                {
                  "module": "test_background",
                  "name": "test_background",
                  "full_name": "test_background::test_background",
                  "status": "error",
                  "duration_seconds": "[TIME]",
                  "diagnostic": {
                    "code": "unhandled-thread-exception",
                    "severity": "error",
                    "message": "Unhandled exception `RuntimeError` in thread `background`",
                    "rendered": "error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`\n  --> test_background.py:28:5\n   |/n28 | def test_background():\n   |     ^^^^^^^^^^^^^^^/ninfo: Exception raised here\n --> test_background.py:7:5\n  |/n7 |     raise RuntimeError(/"background failed/")\n  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: background failed\n\n"
                  },
                  "related_diagnostics": [
                    {
                      "code": "unraisable-exception",
                      "severity": "error",
                      "message": "Unraisable exception `RuntimeError`",
                      "rendered": "error[unraisable-exception]: Unraisable exception `RuntimeError`\n  --> test_background.py:28:5\n   |/n28 | def test_background():\n   |     ^^^^^^^^^^^^^^^/ninfo: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>/ninfo: Object: `None`/ninfo: Exception raised here\n  --> test_background.py:12:9\n   |/n12 |         raise RuntimeError(/"cleanup failed/")\n   |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: cleanup failed\n\n"
                    }
                  ]
                }
              ],
              "run_diagnostics": [
                {
                  "code": "unhandled-thread-exception",
                  "severity": "error",
                  "message": "Unhandled exception `RuntimeError` in thread `background`",
                  "rendered": "error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`/ninfo: Exception raised here\n --> test_background.py:7:5\n  |/n7 |     raise RuntimeError(/"background failed/")\n  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: background failed\n\n"
                },
                {
                  "code": "unraisable-exception",
                  "severity": "error",
                  "message": "Unraisable exception `RuntimeError`",
                  "rendered": "error[unraisable-exception]: Unraisable exception `RuntimeError`/ninfo: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>/ninfo: Object: `None`/ninfo: Exception raised here\n  --> test_background.py:12:9\n   |/n12 |         raise RuntimeError(/"cleanup failed/")\n   |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: cleanup failed\n\n"
                }
              ]
            }
            "#);
        } else {
            assert_snapshot!(context.read_file("results.out"), @r#"
            {"schema_version":2,"type":"test","module":"test_background","name":"test_background","full_name":"test_background::test_background","status":"error","duration_seconds":"[TIME]","diagnostic":{"code":"unhandled-thread-exception","severity":"error","message":"Unhandled exception `RuntimeError` in thread `background`","rendered":"error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`\n  --> test_background.py:28:5\n   |/n28 | def test_background():\n   |     ^^^^^^^^^^^^^^^/ninfo: Exception raised here\n --> test_background.py:7:5\n  |/n7 |     raise RuntimeError(/"background failed/")\n  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: background failed\n\n"},"related_diagnostics":[{"code":"unraisable-exception","severity":"error","message":"Unraisable exception `RuntimeError`","rendered":"error[unraisable-exception]: Unraisable exception `RuntimeError`\n  --> test_background.py:28:5\n   |/n28 | def test_background():\n   |     ^^^^^^^^^^^^^^^/ninfo: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>/ninfo: Object: `None`/ninfo: Exception raised here\n  --> test_background.py:12:9\n   |/n12 |         raise RuntimeError(/"cleanup failed/")\n   |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: cleanup failed\n\n"}]}
            {"schema_version":2,"type":"run_diagnostic","diagnostic":{"code":"unhandled-thread-exception","severity":"error","message":"Unhandled exception `RuntimeError` in thread `background`","rendered":"error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`/ninfo: Exception raised here\n --> test_background.py:7:5\n  |/n7 |     raise RuntimeError(/"background failed/")\n  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: background failed\n\n"}}
            {"schema_version":2,"type":"run_diagnostic","diagnostic":{"code":"unraisable-exception","severity":"error","message":"Unraisable exception `RuntimeError`","rendered":"error[unraisable-exception]: Unraisable exception `RuntimeError`/ninfo: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>/ninfo: Object: `None`/ninfo: Exception raised here\n  --> test_background.py:12:9\n   |/n12 |         raise RuntimeError(/"cleanup failed/")\n   |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: cleanup failed\n\n"}}
            {"schema_version":2,"type":"run_finished","status":"failed","elapsed_seconds":"[TIME]","stats":{"total":1,"passed":0,"failed":0,"errors":1,"skipped":0,"expected_failure":0,"flaky":0,"slow":0}}
            "#);
        }
        assert_snapshot!(context.read_file("results.xml"), @r#"
        <?xml version="1.0" encoding="UTF-8"?>
        <testsuites name="karva-tests" tests="3" failures="0" skipped="0" errors="3" time="[TIME]">
          <testsuite name="test_background" tests="1" failures="0" skipped="0" errors="1" time="[TIME]">
            <testcase classname="test_background" name="test_background" time="[TIME]">
              <error message="Unhandled exception `RuntimeError` in thread `background`" type="unhandled-thread-exception">error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --&gt; test_background.py:28:5
           |
        28 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --&gt; test_background.py:7:5
          |
        7 |     raise RuntimeError(&quot;background failed&quot;)
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --&gt; test_background.py:28:5
           |
        28 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception ignored while calling deallocator &lt;function BrokenCleanup.__del__ at [ADDRESS]&gt;
        info: Object: `None`
        info: Exception raised here
          --&gt; test_background.py:12:9
           |
        12 |         raise RuntimeError(&quot;cleanup failed&quot;)
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        </error>
            </testcase>
          </testsuite>
          <testsuite name="karva-tests::run" tests="2" failures="0" skipped="0" errors="2" time="[TIME]">
            <testcase classname="karva.run" name="unhandled-thread-exception-1" time="[TIME]">
              <error message="Unhandled exception `RuntimeError` in thread `background`" type="unhandled-thread-exception">error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
        info: Exception raised here
         --&gt; test_background.py:7:5
          |
        7 |     raise RuntimeError(&quot;background failed&quot;)
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        </error>
            </testcase>
            <testcase classname="karva.run" name="unraisable-exception-2" time="[TIME]">
              <error message="Unraisable exception `RuntimeError`" type="unraisable-exception">error[unraisable-exception]: Unraisable exception `RuntimeError`
        info: Exception ignored while calling deallocator &lt;function BrokenCleanup.__del__ at [ADDRESS]&gt;
        info: Object: `None`
        info: Exception raised here
          --&gt; test_background.py:12:9
           |
        12 |         raise RuntimeError(&quot;cleanup failed&quot;)
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        </error>
            </testcase>
          </testsuite>
        </testsuites>
        "#);
    } else {
        assert_cmd_snapshot!(command, @r#"
        success: false
        exit_code: 1
        ----- stdout -----

        failures:

        test_background::test_background:

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_background.py:28:5
           |
        28 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --> test_background.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:28:5
           |
        28 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Object: `BrokenCleanup.__del__`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        diagnostics:

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
        info: Exception raised here
         --> test_background.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        error[unraisable-exception]: Unraisable exception `RuntimeError`
        info: Object: `BrokenCleanup.__del__`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
        if format == "json" {
            assert_snapshot!(context.read_file("results.out"), @r#"
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
                "expected_failure": 0,
                "flaky": 0,
                "slow": 0
              },
              "tests": [
                {
                  "module": "test_background",
                  "name": "test_background",
                  "full_name": "test_background::test_background",
                  "status": "error",
                  "duration_seconds": "[TIME]",
                  "diagnostic": {
                    "code": "unhandled-thread-exception",
                    "severity": "error",
                    "message": "Unhandled exception `RuntimeError` in thread `background`",
                    "rendered": "error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`\n  --> test_background.py:28:5\n   |/n28 | def test_background():\n   |     ^^^^^^^^^^^^^^^/ninfo: Exception raised here\n --> test_background.py:7:5\n  |/n7 |     raise RuntimeError(/"background failed/")\n  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: background failed\n\n"
                  },
                  "related_diagnostics": [
                    {
                      "code": "unraisable-exception",
                      "severity": "error",
                      "message": "Unraisable exception `RuntimeError`",
                      "rendered": "error[unraisable-exception]: Unraisable exception `RuntimeError`\n  --> test_background.py:28:5\n   |/n28 | def test_background():\n   |     ^^^^^^^^^^^^^^^/ninfo: Object: `BrokenCleanup.__del__`/ninfo: Exception raised here\n  --> test_background.py:12:9\n   |/n12 |         raise RuntimeError(/"cleanup failed/")\n   |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: cleanup failed\n\n"
                    }
                  ]
                }
              ],
              "run_diagnostics": [
                {
                  "code": "unhandled-thread-exception",
                  "severity": "error",
                  "message": "Unhandled exception `RuntimeError` in thread `background`",
                  "rendered": "error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`/ninfo: Exception raised here\n --> test_background.py:7:5\n  |/n7 |     raise RuntimeError(/"background failed/")\n  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: background failed\n\n"
                },
                {
                  "code": "unraisable-exception",
                  "severity": "error",
                  "message": "Unraisable exception `RuntimeError`",
                  "rendered": "error[unraisable-exception]: Unraisable exception `RuntimeError`/ninfo: Object: `BrokenCleanup.__del__`/ninfo: Exception raised here\n  --> test_background.py:12:9\n   |/n12 |         raise RuntimeError(/"cleanup failed/")\n   |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: cleanup failed\n\n"
                }
              ]
            }
            "#);
        } else {
            assert_snapshot!(context.read_file("results.out"), @r#"
            {"schema_version":2,"type":"test","module":"test_background","name":"test_background","full_name":"test_background::test_background","status":"error","duration_seconds":"[TIME]","diagnostic":{"code":"unhandled-thread-exception","severity":"error","message":"Unhandled exception `RuntimeError` in thread `background`","rendered":"error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`\n  --> test_background.py:28:5\n   |/n28 | def test_background():\n   |     ^^^^^^^^^^^^^^^/ninfo: Exception raised here\n --> test_background.py:7:5\n  |/n7 |     raise RuntimeError(/"background failed/")\n  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: background failed\n\n"},"related_diagnostics":[{"code":"unraisable-exception","severity":"error","message":"Unraisable exception `RuntimeError`","rendered":"error[unraisable-exception]: Unraisable exception `RuntimeError`\n  --> test_background.py:28:5\n   |/n28 | def test_background():\n   |     ^^^^^^^^^^^^^^^/ninfo: Object: `BrokenCleanup.__del__`/ninfo: Exception raised here\n  --> test_background.py:12:9\n   |/n12 |         raise RuntimeError(/"cleanup failed/")\n   |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: cleanup failed\n\n"}]}
            {"schema_version":2,"type":"run_diagnostic","diagnostic":{"code":"unhandled-thread-exception","severity":"error","message":"Unhandled exception `RuntimeError` in thread `background`","rendered":"error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`/ninfo: Exception raised here\n --> test_background.py:7:5\n  |/n7 |     raise RuntimeError(/"background failed/")\n  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: background failed\n\n"}}
            {"schema_version":2,"type":"run_diagnostic","diagnostic":{"code":"unraisable-exception","severity":"error","message":"Unraisable exception `RuntimeError`","rendered":"error[unraisable-exception]: Unraisable exception `RuntimeError`/ninfo: Object: `BrokenCleanup.__del__`/ninfo: Exception raised here\n  --> test_background.py:12:9\n   |/n12 |         raise RuntimeError(/"cleanup failed/")\n   |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^/ninfo: RuntimeError: cleanup failed\n\n"}}
            {"schema_version":2,"type":"run_finished","status":"failed","elapsed_seconds":"[TIME]","stats":{"total":1,"passed":0,"failed":0,"errors":1,"skipped":0,"expected_failure":0,"flaky":0,"slow":0}}
            "#);
        }
        assert_snapshot!(context.read_file("results.xml"), @r#"
        <?xml version="1.0" encoding="UTF-8"?>
        <testsuites name="karva-tests" tests="3" failures="0" skipped="0" errors="3" time="[TIME]">
          <testsuite name="test_background" tests="1" failures="0" skipped="0" errors="1" time="[TIME]">
            <testcase classname="test_background" name="test_background" time="[TIME]">
              <error message="Unhandled exception `RuntimeError` in thread `background`" type="unhandled-thread-exception">error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --&gt; test_background.py:28:5
           |
        28 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --&gt; test_background.py:7:5
          |
        7 |     raise RuntimeError(&quot;background failed&quot;)
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --&gt; test_background.py:28:5
           |
        28 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Object: `BrokenCleanup.__del__`
        info: Exception raised here
          --&gt; test_background.py:12:9
           |
        12 |         raise RuntimeError(&quot;cleanup failed&quot;)
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        </error>
            </testcase>
          </testsuite>
          <testsuite name="karva-tests::run" tests="2" failures="0" skipped="0" errors="2" time="[TIME]">
            <testcase classname="karva.run" name="unhandled-thread-exception-1" time="[TIME]">
              <error message="Unhandled exception `RuntimeError` in thread `background`" type="unhandled-thread-exception">error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
        info: Exception raised here
         --&gt; test_background.py:7:5
          |
        7 |     raise RuntimeError(&quot;background failed&quot;)
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        </error>
            </testcase>
            <testcase classname="karva.run" name="unraisable-exception-2" time="[TIME]">
              <error message="Unraisable exception `RuntimeError`" type="unraisable-exception">error[unraisable-exception]: Unraisable exception `RuntimeError`
        info: Object: `BrokenCleanup.__del__`
        info: Exception raised here
          --&gt; test_background.py:12:9
           |
        12 |         raise RuntimeError(&quot;cleanup failed&quot;)
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        </error>
            </testcase>
          </testsuite>
        </testsuites>
        "#);
    }
}
#[test]
fn hook_restoration_failures_are_run_errors() {
    let context = TestContext::with_file(
        "test_background.py",
        r#"
import sys
import types


class RefusesRestore(types.ModuleType):
    def __setattr__(self, name, value):
        if name == "unraisablehook":
            raise RuntimeError("cannot restore unraisablehook")
        super().__setattr__(name, value)


def test_changes_module():
    sys.__class__ = RefusesRestore
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_background::test_changes_module

    diagnostics:

    error[exception-hook-failure]: Failed to restore Python exception hooks: RuntimeError: cannot restore unraisablehook

    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn errors_during_a_retry_remain_on_that_attempt(#[values("thread", "unraisable")] kind: &str) {
    let context = TestContext::with_file(
        "test_background.py",
        format!(
            r#"{HELPERS}
import os


def test_background():
    if os.environ["KARVA_ATTEMPT"] == "1":
        raise ValueError("retry this ordinary failure")
    emit("{kind}")
"#
        ),
    );
    let _snapshots = background_snapshots();
    if kind == "thread" {
        assert_cmd_snapshot!(context
            .command_no_parallel()
            .args(["--retry=2", "--status-level=none"]), @r#"
        success: false
        exit_code: 1
        ----- stdout -----

        failures:

        test_background::test_background:

        error[unhandled-thread-exception]: Unhandled exception `RuntimeError` in thread `background`
          --> test_background.py:27:5
           |
        27 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception raised here
         --> test_background.py:7:5
          |
        7 |     raise RuntimeError("background failed")
          |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: background failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
    } else if modern_unraisable() {
        assert_cmd_snapshot!(context
            .command_no_parallel()
            .args(["--retry=2", "--status-level=none"]), @r#"
        success: false
        exit_code: 1
        ----- stdout -----

        failures:

        test_background::test_background:

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:27:5
           |
        27 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>
        info: Object: `None`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
    } else {
        assert_cmd_snapshot!(context
            .command_no_parallel()
            .args(["--retry=2", "--status-level=none"]), @r#"
        success: false
        exit_code: 1
        ----- stdout -----

        failures:

        test_background::test_background:

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:27:5
           |
        27 | def test_background():
           |     ^^^^^^^^^^^^^^^
        info: Object: `BrokenCleanup.__del__`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
    }
}

#[test]
fn fixture_value_destructors_belong_to_teardown() {
    let context = TestContext::with_file(
        "test_background.py",
        format!(
            r#"{HELPERS}
class FixtureValue(BrokenCleanup):
    def __repr__(self):
        return "resource"


@karva.fixture
def resource():
    return FixtureValue()


def test_background(resource):
    pass
"#
        ),
    );
    let _snapshots = background_snapshots();
    if modern_unraisable() {
        assert_cmd_snapshot!(context.command_no_parallel(), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test_background::test_background(resource=resource)

        failures:

        test_background::test_background(resource=resource):

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:34:5
           |
        34 | def test_background(resource):
           |     ^^^^^^^^^^^^^^^
        info: Exception ignored while calling deallocator <function BrokenCleanup.__del__ at [ADDRESS]>
        info: Object: `None`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
    } else {
        assert_cmd_snapshot!(context.command_no_parallel(), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 1 test across 1 worker
               ERROR [TIME] test_background::test_background(resource=resource)

        failures:

        test_background::test_background(resource=resource):

        error[unraisable-exception]: Unraisable exception `RuntimeError`
          --> test_background.py:34:5
           |
        34 | def test_background(resource):
           |     ^^^^^^^^^^^^^^^
        info: Object: `BrokenCleanup.__del__`
        info: Exception raised here
          --> test_background.py:12:9
           |
        12 |         raise RuntimeError("cleanup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: RuntimeError: cleanup failed

        ────────────
             Summary [TIME] 1 test run: 0 passed, 1 error, 0 skipped

        ----- stderr -----
        "#);
    }
}
