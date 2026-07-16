//! Extended coverage for framework fixtures.
//!
//! These tests complement `builtins.rs` with precedence / shadowing
//! regressions, multi-logger caplog behaviour, additional capsys and recwarn
//! edge cases, `TempPathFactory` numbered-dir semantics, and extra monkeypatch
//! scenarios around descriptors and context managers. They exist primarily to
//! lock in behaviour that the framework-fixture rewrite could regress without
//! user-visible symptoms.

use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

// =============================================================================
// Precedence: user conftest shadows framework fixture with the same name.
// =============================================================================

#[test]
fn test_user_conftest_shadows_framework_tmp_path() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import karva
from pathlib import Path

@karva.fixture
def tmp_path():
    return Path("/shadow/path")
"#,
        ),
        (
            "test_shadow.py",
            r#"
from pathlib import Path

def test_uses_user_tmp_path(tmp_path):
    assert tmp_path == Path("/shadow/path")
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_shadow::test_uses_user_tmp_path(tmp_path=/shadow/path)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_user_conftest_shadows_framework_monkeypatch() {
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import karva

class FakeMonkeypatch:
    def __repr__(self):
        return "<user fake monkeypatch>"

@karva.fixture
def monkeypatch():
    return FakeMonkeypatch()
"#,
        ),
        (
            "test_shadow.py",
            r#"
def test_uses_user_monkeypatch(monkeypatch):
    assert repr(monkeypatch) == "<user fake monkeypatch>"
"#,
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_shadow::test_uses_user_monkeypatch(monkeypatch=<user fake monkeypatch>)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_framework_fixture_visible_without_conftest() {
    let context = TestContext::with_file(
        "test_framework.py",
        r"
from pathlib import Path

def test_tmp_path_still_works(tmp_path, monkeypatch):
    assert isinstance(tmp_path, Path)
    assert tmp_path.exists()
    assert tmp_path.is_dir()
",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_framework::test_tmp_path_still_works(tmp_path, monkeypatch)
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

// =============================================================================
// caplog: multi-logger set_level restore (regression for the bug where only
// the first logger touched by `set_level` was restored on teardown).
// =============================================================================

#[test]
fn test_caplog_set_level_restores_all_touched_loggers() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import logging

import karva


def test_first(caplog):
    logging.getLogger("pkg.a").setLevel(logging.WARNING)
    logging.getLogger("pkg.b").setLevel(logging.WARNING)
    caplog.set_level(logging.DEBUG, logger="pkg.a")
    caplog.set_level(logging.INFO, logger="pkg.b")


def test_second_sees_restored_levels():
    assert logging.getLogger("pkg.a").level == logging.WARNING
    assert logging.getLogger("pkg.b").level == logging.WARNING
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_caplog_at_level_nested() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import logging

def test_at_level_nested(caplog):
    pkg = logging.getLogger("pkg.a")
    pkg.setLevel(logging.WARNING)

    with caplog.at_level(logging.DEBUG, logger="pkg.a"):
        assert pkg.level == logging.DEBUG
        with caplog.at_level(logging.INFO, logger="pkg.a"):
            assert pkg.level == logging.INFO
        assert pkg.level == logging.DEBUG

    assert pkg.level == logging.WARNING
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_caplog_captures_from_child_logger() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import logging

def test_child_logger_propagates(caplog):
    logging.getLogger("parent.child").warning("hello from child")
    names = [r.name for r in caplog.records]
    assert "parent.child" in names
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_caplog_records_exception_info() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import logging

def test_exc_info(caplog):
    logger = logging.getLogger("exc_test")
    try:
        raise ValueError("boom")
    except ValueError:
        logger.exception("caught")

    assert len(caplog.records) == 1
    record = caplog.records[0]
    assert record.exc_info is not None
    assert record.exc_info[0] is ValueError
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

// =============================================================================
// capsys: additional edge cases.
// =============================================================================

#[test]
fn test_capsys_multiple_readouterr_calls() {
    let context = TestContext::with_file(
        "test.py",
        r#"
def test_readouterr_segments(capsys):
    print("first")
    first = capsys.readouterr()
    assert first.out == "first\n"

    print("second")
    second = capsys.readouterr()
    assert second.out == "second\n"

    third = capsys.readouterr()
    assert third.out == ""
    assert third.err == ""
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_capsys_stdout_and_stderr_separate() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import sys

def test_streams_are_separate(capsys):
    print("to out")
    print("to err", file=sys.stderr)
    captured = capsys.readouterr()
    assert captured.out == "to out\n"
    assert captured.err == "to err\n"
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_capsys_unicode_payload() {
    let context = TestContext::with_file(
        "test.py",
        r#"
def test_unicode(capsys):
    print("héllo wörld")
    assert capsys.readouterr().out == "héllo wörld\n"
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_capsys_result_named_tuple_unpack() {
    let context = TestContext::with_file(
        "test.py",
        r#"
def test_named_tuple(capsys):
    print("value", end="")
    out, err = capsys.readouterr()
    assert out == "value"
    assert err == ""
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

// =============================================================================
// recwarn: more filtering and iteration cases.
// =============================================================================

#[test]
fn test_recwarn_filter_by_category() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import warnings

def test_filter_category(recwarn):
    warnings.warn("first", UserWarning)
    warnings.warn("second", DeprecationWarning)
    warnings.warn("third", UserWarning)

    user_warnings = [w for w in recwarn if issubclass(w.category, UserWarning)]
    assert len(user_warnings) == 2
    assert str(user_warnings[0].message) == "first"
    assert str(user_warnings[1].message) == "third"
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_recwarn_message_attributes() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import warnings

def test_message_attrs(recwarn):
    warnings.warn("hello", RuntimeWarning)
    assert len(recwarn) == 1
    w = recwarn[0]
    assert w.category is RuntimeWarning
    assert str(w.message) == "hello"
    assert w.filename is not None
    assert isinstance(w.lineno, int)
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_recwarn_pop_finds_subclass() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import warnings

class CustomWarning(UserWarning):
    pass

def test_pop_subclass(recwarn):
    warnings.warn("user", UserWarning)
    warnings.warn("custom", CustomWarning)

    popped = recwarn.pop(UserWarning)
    assert str(popped.message) in {"user", "custom"}
    assert len(recwarn) == 1
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

// =============================================================================
// TempPathFactory: numbered-dir semantics and relative-path guard.
// =============================================================================

#[test]
fn test_tmp_path_factory_numbered_dirs_unique() {
    let context = TestContext::with_file(
        "test.py",
        r#"
def test_numbered_unique(tmp_path_factory):
    d1 = tmp_path_factory.mktemp("x")
    d2 = tmp_path_factory.mktemp("x")
    d3 = tmp_path_factory.mktemp("x")
    dirs = {str(d1), str(d2), str(d3)}
    assert len(dirs) == 3
    for d in (d1, d2, d3):
        assert d.exists() and d.is_dir()
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_tmp_path_factory_numbered_false_no_suffix() {
    let context = TestContext::with_file(
        "test.py",
        r#"
def test_unnumbered(tmp_path_factory):
    d = tmp_path_factory.mktemp("only", numbered=False)
    assert d.name == "only"
    assert d.exists() and d.is_dir()
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_tmp_path_factory_rejects_escaping_basename() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva

def test_escaping(tmp_path_factory):
    with karva.raises(ValueError, match="not a normalized and relative path"):
        tmp_path_factory.mktemp("../escape")
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_tmp_path_factory_basetemp_is_writable_dir() {
    let context = TestContext::with_file(
        "test.py",
        r#"
def test_basetemp(tmp_path_factory):
    base = tmp_path_factory.getbasetemp()
    assert base.exists()
    assert base.is_dir()
    probe = base / "probe.txt"
    probe.write_text("ok")
    assert probe.read_text() == "ok"
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_tmp_path_factory_shared_across_tests() {
    let context = TestContext::with_file(
        "test.py",
        r"
seen = []

def test_a(tmp_path_factory):
    seen.append(tmp_path_factory.getbasetemp())

def test_b(tmp_path_factory):
    seen.append(tmp_path_factory.getbasetemp())
    assert len(seen) == 2
    assert seen[0] == seen[1]
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

// =============================================================================
// MockEnv as a direct context manager (karva-only adaptation; not in pytest).
// =============================================================================

#[test]
fn test_mockenv_instance_as_with_statement() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os

import karva


def test_mockenv_with():
    os.environ["KARVA_MOCK_SMOKE"] = "before"
    try:
        with karva.MockEnv() as mp:
            mp.setenv("KARVA_MOCK_SMOKE", "during")
            assert os.environ["KARVA_MOCK_SMOKE"] == "during"
        assert os.environ["KARVA_MOCK_SMOKE"] == "before"
    finally:
        del os.environ["KARVA_MOCK_SMOKE"]
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_mockenv_undo_twice_is_idempotent() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva


class A:
    x = 1


def test_double_undo():
    mp = karva.MockEnv()
    mp.setattr(A, "x", 99)
    assert A.x == 99
    mp.undo()
    assert A.x == 1
    mp.undo()
    assert A.x == 1
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

// =============================================================================
// Extra monkeypatch: descriptor handling, pathlike chdir, prepend semantics.
// =============================================================================

#[test]
fn test_monkeypatch_undo_staticmethod_via_class() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva


class Service:
    @staticmethod
    def ping():
        return "pong"


def test_staticmethod_restore():
    mp = karva.MockEnv()
    mp.setattr(Service, "ping", staticmethod(lambda: "replaced"))
    assert Service.ping() == "replaced"
    mp.undo()
    assert Service.ping() == "pong"
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_monkeypatch_undo_classmethod_via_class() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva


class Service:
    @classmethod
    def tag(cls):
        return cls.__name__


def test_classmethod_restore():
    mp = karva.MockEnv()
    mp.setattr(Service, "tag", classmethod(lambda _cls: "shadowed"))
    assert Service.tag() == "shadowed"
    mp.undo()
    assert Service.tag() == "Service"
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_monkeypatch_chdir_with_pathlib_object() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os
from pathlib import Path

import karva


def test_chdir_pathlib(tmp_path):
    cwd_before = os.getcwd()
    mp = karva.MockEnv()
    mp.chdir(Path(tmp_path))
    try:
        assert os.path.realpath(os.getcwd()) == os.path.realpath(str(tmp_path))
    finally:
        mp.undo()
    assert os.getcwd() == cwd_before
",
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_monkeypatch_setenv_prepend_chains() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os

import karva


def test_prepend():
    key = "KARVA_PATH_PROBE"
    try:
        mp = karva.MockEnv()
        mp.setenv(key, "a")
        mp.setenv(key, "b", prepend=":")
        mp.setenv(key, "c", prepend=":")
        assert os.environ[key] == "c:b:a"
        mp.undo()
        assert key not in os.environ
    finally:
        os.environ.pop(key, None)
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_monkeypatch_setattr_undo_of_absent_attr() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva


class Target:
    pass


def test_setattr_missing_then_undo():
    mp = karva.MockEnv()
    mp.setattr(Target, "new_attr", 42, raising=False)
    assert Target.new_attr == 42
    mp.undo()
    assert not hasattr(Target, "new_attr")
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_monkeypatch_delitem_twice_raises() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva


def test_delitem_missing():
    d = {"x": 1}
    mp = karva.MockEnv()
    mp.delitem(d, "x")
    with karva.raises(KeyError):
        mp.delitem(d, "x")
    mp.delitem(d, "x", raising=False)
    mp.undo()
    assert d == {"x": 1}
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

// =============================================================================
// Framework fixture error diagnostic: when a test depends on something that
// doesn't exist, the missing-fixture diagnostic still surfaces correctly.
// =============================================================================

#[test]
fn test_unknown_fixture_name_produces_missing_diagnostic() {
    let context = TestContext::with_file(
        "test.py",
        r"
def test_no_such_fixture(not_a_real_fixture):
    pass
",
    );

    // Intentionally use the full (non-quiet) output so that the missing-fixture
    // diagnostic is captured in the snapshot — `-q` elides per-test diagnostics.
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_no_such_fixture

    diagnostics:

    error[missing-fixtures]: Test `test_no_such_fixture` has missing fixtures
     --> test.py:2:5
      |
    2 | def test_no_such_fixture(not_a_real_fixture):
      |     ^^^^^^^^^^^^^^^^^^^^
      |
    info: Missing fixtures: `not_a_real_fixture`

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_session_autouse_can_depend_on_framework_fixture() {
    // Regression for the session autouse gate: a user-defined session-scope
    // autouse fixture that depends on a framework fixture (`tmp_path_factory`)
    // must resolve correctly. Before the gate was dropped the session-autouse
    // walk only looked at a user conftest's fixtures, so dependency resolution
    // could not reach `framework_module`.
    let context = TestContext::with_files([
        (
            "conftest.py",
            r#"
import karva

@karva.fixture(scope="session", auto_use=True)
def session_autouse(tmp_path_factory):
    tmp_path_factory.mktemp("session-auto")
    return "ran"
"#,
        ),
        (
            "test_session.py",
            r"
def test_ran():
    pass
",
        ),
    ]);

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test_session::test_ran
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_derive_importpath_regression_from_pytest_issue_1338() {
    // Regression equivalent to pytest's `test_issue1338_name_resolving` — the
    // `delattr("dotted.import.path")` form must successfully traverse a
    // package boundary where the final attribute is defined by a module whose
    // parent is another module. We substitute `os.path.defpath` for pytest's
    // `requests.sessions.Session.request` so the test does not require an
    // external dependency: `os.path` is itself a submodule resolved via
    // `importlib.import_module` inside `derive_importpath`, giving the same
    // resolution path.
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva


def test_delattr_resolves_submodule_attr():
    mp = karva.MockEnv()
    try:
        mp.delattr("os.path.defpath")
        import os.path as ospath
        assert not hasattr(ospath, "defpath")
    finally:
        mp.undo()
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}
