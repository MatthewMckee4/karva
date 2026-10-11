//! Exceptions escaping Python calls must survive lifecycle and report boundaries.

use insta::{assert_snapshot, with_settings};
use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

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
        &format!(
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
    with_settings!({snapshot_suffix => format!("{kind}_{phase}")}, {
        assert_cmd_snapshot!(context.command_no_parallel());
    });
}

#[test]
fn retains_multiple_events_and_existing_failure() {
    let context = TestContext::with_file(
        "test_background.py",
        &format!(
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
    assert_cmd_snapshot!(context.command_no_parallel());
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
        &format!(
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
    with_settings!({snapshot_suffix => format!("{kind}_{phase}")}, {
        assert_cmd_snapshot!(context.command_no_parallel());
    });
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
    assert_cmd_snapshot!(context.command_no_parallel());
}

#[rstest]
fn retries_cannot_hide_background_errors(#[values("thread", "unraisable")] kind: &str) {
    let context = TestContext::with_file(
        "test_background.py",
        &format!(
            r#"{HELPERS}
import os


def test_background():
    if os.environ["KARVA_ATTEMPT"] == "1":
        emit("{kind}")
"#
        ),
    );
    with_settings!({snapshot_suffix => kind}, {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--retry=1"));
    });
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
    assert_cmd_snapshot!(
        context
            .command()
            .args(["--num-workers=2", "--status-level=none"])
    );
}

#[test]
fn background_errors_override_skip_and_expected_failure() {
    let context = TestContext::with_file(
        "test_background.py",
        &format!(
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
    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"));
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
    assert_cmd_snapshot!(context.command_no_parallel());
}

#[rstest]
fn machine_reports_retain_test_and_worker_events(#[values("json", "jsonl")] format: &str) {
    let context = TestContext::with_file(
        "test_background.py",
        &format!(
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
    with_settings!({snapshot_suffix => format}, {
        assert_cmd_snapshot!(context.command_no_parallel().args([
            "--status-level=none",
            "--result-output=results.out",
            &format!("--result-format={format}"),
        ]));
        assert_snapshot!("machine_report", context.read_file("results.out"));
        // JUnit timings do not use the JSON normalization supplied by TestContext.
        with_settings!({filters => vec![(r#"time="[0-9.]+""#, "time=\"[TIME]\"")]}, {
            assert_snapshot!("junit_report", context.read_file("results.xml"));
        });
    });
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
    assert_cmd_snapshot!(context.command_no_parallel());
}

#[rstest]
fn errors_during_a_retry_remain_on_that_attempt(#[values("thread", "unraisable")] kind: &str) {
    let context = TestContext::with_file(
        "test_background.py",
        &format!(
            r#"{HELPERS}
import os


def test_background():
    if os.environ["KARVA_ATTEMPT"] == "1":
        raise ValueError("retry this ordinary failure")
    emit("{kind}")
"#
        ),
    );
    with_settings!({snapshot_suffix => kind}, {
        assert_cmd_snapshot!(context.command_no_parallel().args(["--retry=2", "--status-level=none"]));
    });
}

#[test]
fn fixture_value_destructors_belong_to_teardown() {
    let context = TestContext::with_file(
        "test_background.py",
        &format!(
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
    assert_cmd_snapshot!(context.command_no_parallel());
}
