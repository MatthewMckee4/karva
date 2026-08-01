//! Tests adapted from pytest's `testing/test_recwarn.py` and
//! `testing/test_tmpdir.py` (commit `8ecf49ec2`), focused on the subset of
//! pytest behavior maintained by Karva's fixture support:
//!
//! - `WarningsRecorder` semantics (`_pytest/recwarn.py`),
//! - `make_numbered_dir` / `cleanup_numbered_dir` / `rm_rf` from
//!   `_pytest/pathlib.py`,
//! - the `TempPathFactory.mktemp` naming contract from `_pytest/tmpdir.py`.
//!
//! Each Rust test wraps one or more near-verbatim Python test bodies inside a
//! `TestContext::with_file` harness so the assertions run through karva's own
//! test runner — this gives coverage of the internal modules as they are
//! actually imported at runtime, not via Rust unit tests that sidestep the
//! wheel layout.
//!
//! Tests from pytest that depend on `pytester`, `WarningsChecker`,
//! `pytest.warns`, the `Config`-backed `TempPathFactory.from_config`, or the
//! `--basetemp` / retention-policy machinery are intentionally not ported,
//! because karva does not expose those entry points.
//!
//! See the pytest license block in the repository `LICENSE` file for the
//! applicable copyright notice.

use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

// =============================================================================
// WarningsRecorder (adapted from pytest's TestWarningsRecorderChecker).
// =============================================================================

#[test]
fn test_warnings_recorder_recording_lifecycle() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import warnings

import karva
from karva._fixtures.recwarn import WarningsRecorder


def test_recording():
    rec = WarningsRecorder()
    with rec:
        assert not rec.list
        warnings.warn_explicit("hello", UserWarning, "xyz", 13)
        assert len(rec.list) == 1
        warnings.warn(DeprecationWarning("hello"))
        assert len(rec.list) == 2
        popped = rec.pop()
        assert str(popped.message) == "hello"
        values = rec.list
        rec.clear()
        assert len(rec.list) == 0
        assert values is rec.list
        with karva.raises(AssertionError):
            rec.pop()
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
fn test_warnings_recorder_invalid_enter_exit() {
    let context = TestContext::with_file(
        "test.py",
        r"
import karva
from karva._fixtures.recwarn import WarningsRecorder


def test_invalid_enter_exit():
    with WarningsRecorder():
        with karva.raises(RuntimeError):
            rec = WarningsRecorder()
            rec.__exit__(None, None, None)

        with karva.raises(RuntimeError):
            rec = WarningsRecorder()
            with rec:
                with rec:
                    pass
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
fn test_warnings_recorder_captures_deprecation_warning() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import warnings


def test_captures_deprecation_warning(recwarn):
    warnings.warn("dep", DeprecationWarning)
    assert len(recwarn) == 1
    assert issubclass(recwarn[0].category, DeprecationWarning)
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
// Numbered-dir helpers from `_pytest/pathlib.py` (adapted from TestNumberedDir).
// =============================================================================

#[test]
fn test_numbered_dir_make_increments_suffix() {
    let context = TestContext::with_file(
        "test.py",
        r#"
from karva._fixtures.pathlib import make_numbered_dir


def test_make_numbered(tmp_path):
    prefix = "fun-"
    last = None
    for i in range(10):
        d = make_numbered_dir(root=tmp_path, prefix=prefix)
        assert d.name.startswith(prefix)
        assert d.name.endswith(str(i))
        last = d

    symlink = tmp_path / (prefix + "current")
    if symlink.exists():
        assert symlink.is_symlink()
        assert symlink.resolve() == last.resolve()
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
fn test_numbered_dir_cleanup_lock_single_owner() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva
from karva._fixtures.pathlib import create_cleanup_lock


def test_cleanup_lock_create(tmp_path):
    d = tmp_path / "test"
    d.mkdir()
    lock = create_cleanup_lock(d)
    with karva.raises(OSError, match="cannot create lockfile"):
        create_cleanup_lock(d)
    lock.unlink()
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
fn test_numbered_dir_cleanup_keep() {
    let context = TestContext::with_file(
        "test.py",
        r#"
from karva._fixtures.pathlib import cleanup_numbered_dir, make_numbered_dir


PREFIX = "fun-"


def _populate(tmp_path):
    for _ in range(10):
        make_numbered_dir(root=tmp_path, prefix=PREFIX)


def test_cleanup_keeps_last_two(tmp_path):
    _populate(tmp_path)
    cleanup_numbered_dir(
        root=tmp_path,
        prefix=PREFIX,
        keep=2,
        consider_lock_dead_if_created_before=0,
    )
    dirs = sorted(
        x.name for x in tmp_path.iterdir()
        if x.name.startswith(PREFIX) and not x.is_symlink()
    )
    assert dirs == [f"{PREFIX}{i}" for i in (8, 9)]


def test_cleanup_keeps_zero(tmp_path):
    _populate(tmp_path)
    cleanup_numbered_dir(
        root=tmp_path,
        prefix=PREFIX,
        keep=0,
        consider_lock_dead_if_created_before=0,
    )
    assert not [
        x for x in tmp_path.iterdir()
        if x.name.startswith(PREFIX) and not x.is_symlink()
    ]
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
fn test_numbered_dir_ensure_deletable_respects_lock() {
    let context = TestContext::with_file(
        "test.py",
        r#"
from karva._fixtures.pathlib import (
    create_cleanup_lock,
    ensure_deletable,
    make_numbered_dir,
)


def test_ensure_deletable(tmp_path):
    p = make_numbered_dir(root=tmp_path, prefix="fun-")
    create_cleanup_lock(p)

    assert not ensure_deletable(
        p, consider_lock_dead_if_created_before=p.stat().st_mtime - 1
    )
    assert ensure_deletable(
        p, consider_lock_dead_if_created_before=p.stat().st_mtime + 1
    )
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
fn test_numbered_dir_maybe_delete_respects_lock() {
    let context = TestContext::with_file(
        "test.py",
        r#"
from karva._fixtures.pathlib import (
    create_cleanup_lock,
    make_numbered_dir,
    maybe_delete_a_numbered_dir,
)


def test_maybe_delete_respects_lock(tmp_path):
    d = make_numbered_dir(root=tmp_path, prefix="fun-")
    create_cleanup_lock(d)
    maybe_delete_a_numbered_dir(d)
    assert d.is_dir()
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
// rm_rf (adapted from TestRmRf).
// =============================================================================

#[test]
fn test_rm_rf_removes_directory() {
    let context = TestContext::with_file(
        "test.py",
        r#"
from karva._fixtures.pathlib import rm_rf


def test_rm_rf_empty(tmp_path):
    target = tmp_path / "adir"
    target.mkdir()
    rm_rf(target)
    assert not target.exists()


def test_rm_rf_nested(tmp_path):
    target = tmp_path / "adir"
    target.mkdir()
    (target / "afile").write_bytes(b"aa")
    sub = target / "inner"
    sub.mkdir()
    (sub / "other").write_text("hi")
    rm_rf(target)
    assert not target.exists()
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
fn test_rm_rf_read_only_file() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import stat

from karva._fixtures.pathlib import rm_rf


def test_rm_rf_read_only_file(tmp_path):
    target = tmp_path / "ro"
    target.mkdir()
    file = target / "file"
    file.write_bytes(b"data")
    file.chmod(stat.S_IREAD)
    try:
        rm_rf(target)
        assert not target.exists()
    finally:
        # Restore permissions if rm_rf didn't actually remove (e.g. on a
        # filesystem that cannot strip the read-only bit) so the test cleanup
        # in `tmp_path` can still remove them.
        if target.exists():
            file.chmod(stat.S_IRWXU)
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
// TempPathFactory mktemp naming (adapted from TestTmpPathHandler::test_mktemp).
// =============================================================================

#[test]
fn test_temp_path_factory_mktemp_numbering() {
    let context = TestContext::with_file(
        "test.py",
        r#"
from karva._fixtures.tmpdir import TempPathFactory


def test_mktemp_naming(tmp_path):
    factory = TempPathFactory(given_basetemp=tmp_path / "base")
    assert factory.getbasetemp().is_dir()

    one = factory.mktemp("world")
    assert str(one.relative_to(factory.getbasetemp())) == "world0"

    two = factory.mktemp("this")
    assert str(two.relative_to(factory.getbasetemp())).startswith("this")

    three = factory.mktemp("this")
    assert str(three.relative_to(factory.getbasetemp())).startswith("this")
    assert two != three
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
fn test_temp_path_factory_given_basetemp_cleared_on_reuse() {
    let context = TestContext::with_file(
        "test.py",
        r#"
from karva._fixtures.tmpdir import TempPathFactory


def test_reused_basetemp_is_wiped(tmp_path):
    base = tmp_path / "base"
    base.mkdir()
    (base / "stale").write_text("old")

    factory = TempPathFactory(given_basetemp=base)
    factory.getbasetemp()

    assert not (base / "stale").exists()
    assert base.is_dir()
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
