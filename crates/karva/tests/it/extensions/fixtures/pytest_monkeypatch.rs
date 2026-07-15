//! Tests adapted from pytest's `testing/test_monkeypatch.py` (commit
//! `8ecf49ec2`). The Python test bodies are copied near-verbatim from pytest
//! with only the following substitutions:
//!
//! - `MonkeyPatch()` -> `karva.MockEnv()`
//! - `pytest.raises(...)` -> `karva.raises(...)`
//! - `@pytest.mark.parametrize(...)` -> `@karva.tags.parametrize(...)`
//! - `pytest.warns(pytest.PytestWarning)` cases are rewritten to use
//!   `warnings.catch_warnings` since karva does not wrap the vendored
//!   `setenv` warning in a framework-specific category.
//!
//! Tests that depended on `pytester`, the `_pytest.config` internals, or the
//! legacy `pkg_resources` namespace-package path are not ported because the
//! corresponding features are either not available in karva or were dropped
//! when vendoring `MonkeyPatch`.
//!
//! See the pytest license block in the repository LICENSE file for the
//! applicable copyright notice.

use insta_cmd::assert_cmd_snapshot;

use crate::common::TestContext;

#[test]
fn test_setattr_class_attribute() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva


def test_setattr():
    class A:
        x = 1

    monkeypatch = karva.MockEnv()
    with karva.raises(AttributeError):
        monkeypatch.setattr(A, "notexists", 2)
    monkeypatch.setattr(A, "y", 2, raising=False)
    assert A.y == 2
    monkeypatch.undo()
    assert not hasattr(A, "y")

    monkeypatch = karva.MockEnv()
    monkeypatch.setattr(A, "x", 2)
    assert A.x == 2
    monkeypatch.setattr(A, "x", 3)
    assert A.x == 3
    monkeypatch.undo()
    assert A.x == 1

    A.x = 5
    monkeypatch.undo()
    assert A.x == 5

    with karva.raises(TypeError):
        monkeypatch.setattr(A, "y")
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_setattr
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_setattr_with_import_path() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os

import karva


def test_string_expression(monkeypatch):
    with monkeypatch.context() as mp:
        mp.setattr("os.path.abspath", lambda x: "hello2")
        assert os.path.abspath("123") == "hello2"


def test_wrong_target(monkeypatch):
    with karva.raises(TypeError):
        monkeypatch.setattr(None, None)


def test_unknown_import(monkeypatch):
    with karva.raises(ImportError):
        monkeypatch.setattr("unkn123.classx", None)


def test_unknown_attr(monkeypatch):
    with karva.raises(AttributeError):
        monkeypatch.setattr("os.path.qweqwe", None)


def test_unknown_attr_non_raising(monkeypatch):
    with monkeypatch.context() as mp:
        mp.setattr("os.path.qweqwe", 42, raising=False)
        assert os.path.qweqwe == 42


def test_delattr_import_path(monkeypatch):
    with monkeypatch.context() as mp:
        mp.delattr("os.path.abspath")
        assert not hasattr(os.path, "abspath")
        mp.undo()
        assert os.path.abspath
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 6 tests across 1 worker
            PASS [TIME] test::test_string_expression(monkeypatch)
            PASS [TIME] test::test_wrong_target(monkeypatch)
            PASS [TIME] test::test_unknown_import(monkeypatch)
            PASS [TIME] test::test_unknown_attr(monkeypatch)
            PASS [TIME] test::test_unknown_attr_non_raising(monkeypatch)
            PASS [TIME] test::test_delattr_import_path(monkeypatch)
    ────────────
         Summary [TIME] 6 tests run: 6 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_delattr_class_attribute() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva


def test_delattr():
    class A:
        x = 1

    monkeypatch = karva.MockEnv()
    monkeypatch.delattr(A, "x")
    assert not hasattr(A, "x")
    monkeypatch.undo()
    assert A.x == 1

    monkeypatch = karva.MockEnv()
    monkeypatch.delattr(A, "x")
    with karva.raises(AttributeError):
        monkeypatch.delattr(A, "y")
    monkeypatch.delattr(A, "y", raising=False)
    monkeypatch.setattr(A, "x", 5, raising=False)
    assert A.x == 5
    monkeypatch.undo()
    assert A.x == 1
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_delattr
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_setitem_and_delitem() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva


def test_setitem():
    d = {"x": 1}
    monkeypatch = karva.MockEnv()
    monkeypatch.setitem(d, "x", 2)
    monkeypatch.setitem(d, "y", 1700)
    monkeypatch.setitem(d, "y", 1700)
    assert d["x"] == 2
    assert d["y"] == 1700
    monkeypatch.setitem(d, "x", 3)
    assert d["x"] == 3
    monkeypatch.undo()
    assert d["x"] == 1
    assert "y" not in d
    d["x"] = 5
    monkeypatch.undo()
    assert d["x"] == 5


def test_setitem_deleted_meanwhile():
    d = {}
    monkeypatch = karva.MockEnv()
    monkeypatch.setitem(d, "x", 2)
    del d["x"]
    monkeypatch.undo()
    assert not d


def test_delitem():
    d = {"x": 1}
    monkeypatch = karva.MockEnv()
    monkeypatch.delitem(d, "x")
    assert "x" not in d
    monkeypatch.delitem(d, "y", raising=False)
    with karva.raises(KeyError):
        monkeypatch.delitem(d, "y")
    assert not d
    monkeypatch.setitem(d, "y", 1700)
    assert d["y"] == 1700
    d["hello"] = "world"
    monkeypatch.setitem(d, "x", 1500)
    assert d["x"] == 1500
    monkeypatch.undo()
    assert d == {"hello": "world", "x": 1}
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_setitem
            PASS [TIME] test::test_setitem_deleted_meanwhile
            PASS [TIME] test::test_delitem
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_setenv_and_delenv() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os

import karva


@karva.tags.parametrize("before", [True, False])
def test_setenv_deleted_meanwhile(before):
    key = "qwpeoip123"
    if before:
        os.environ[key] = "world"
    monkeypatch = karva.MockEnv()
    monkeypatch.setenv(key, "hello")
    del os.environ[key]
    monkeypatch.undo()
    if before:
        assert os.environ[key] == "world"
        del os.environ[key]
    else:
        assert key not in os.environ


def test_delenv():
    name = "xyz1234"
    assert name not in os.environ
    monkeypatch = karva.MockEnv()
    with karva.raises(KeyError):
        monkeypatch.delenv(name, raising=True)
    monkeypatch.delenv(name, raising=False)
    monkeypatch.undo()
    os.environ[name] = "1"
    try:
        monkeypatch = karva.MockEnv()
        monkeypatch.delenv(name)
        assert name not in os.environ
        monkeypatch.setenv(name, "3")
        assert os.environ[name] == "3"
        monkeypatch.undo()
        assert os.environ[name] == "1"
    finally:
        if name in os.environ:
            del os.environ[name]


def test_setenv_prepend():
    monkeypatch = karva.MockEnv()
    monkeypatch.setenv("XYZ123", "2", prepend="-")
    monkeypatch.setenv("XYZ123", "3", prepend="-")
    assert os.environ["XYZ123"] == "3-2"
    monkeypatch.undo()
    assert "XYZ123" not in os.environ
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_setenv_deleted_meanwhile(before=True)
            PASS [TIME] test::test_setenv_deleted_meanwhile(before=False)
            PASS [TIME] test::test_delenv
            PASS [TIME] test::test_setenv_prepend
    ────────────
         Summary [TIME] 4 tests run: 4 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_setenv_non_str_warning() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import warnings

import karva


def test_setenv_non_str_warning():
    monkeypatch = karva.MockEnv()
    with warnings.catch_warnings(record=True) as caught:
        warnings.simplefilter("always")
        monkeypatch.setenv("PYTEST_INTERNAL_MY_VAR", 2)
    assert any(
        "should be str" in str(w.message)
        and "PYTEST_INTERNAL_MY_VAR" in str(w.message)
        for w in caught
    )
    assert os.environ["PYTEST_INTERNAL_MY_VAR"] == "2"
    monkeypatch.undo()


import os
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_setenv_non_str_warning
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_syspath_prepend_group() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
import sys

import karva


@karva.fixture
def mp():
    cwd = os.getcwd()
    sys_path = list(sys.path)
    yield karva.MockEnv()
    sys.path[:] = sys_path
    os.chdir(cwd)


def test_syspath_prepend(mp):
    old = list(sys.path)
    mp.syspath_prepend("world")
    mp.syspath_prepend("hello")
    assert sys.path[0] == "hello"
    assert sys.path[1] == "world"
    mp.undo()
    assert sys.path == old
    mp.undo()
    assert sys.path == old


def test_syspath_prepend_double_undo(mp):
    old_syspath = sys.path[:]
    try:
        mp.syspath_prepend("hello world")
        mp.undo()
        sys.path.append("more hello world")
        mp.undo()
        assert sys.path[-1] == "more hello world"
    finally:
        sys.path[:] = old_syspath
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_syspath_prepend(mp=<MockEnv object>)
            PASS [TIME] test::test_syspath_prepend_double_undo(mp=<MockEnv object>)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_chdir_group() {
    let context = TestContext::with_file(
        "test.py",
        r"
import os
import tempfile
from pathlib import Path

import karva


@karva.fixture
def mp():
    cwd = os.getcwd()
    yield karva.MockEnv()
    os.chdir(cwd)


def _tmp_dir():
    return Path(tempfile.mkdtemp()).resolve()


def test_chdir_with_path_local(mp):
    tmp_path = _tmp_dir()
    mp.chdir(tmp_path)
    assert os.getcwd() == str(tmp_path)


def test_chdir_with_str(mp):
    tmp_path = _tmp_dir()
    mp.chdir(str(tmp_path))
    assert os.getcwd() == str(tmp_path)


def test_chdir_undo(mp):
    tmp_path = _tmp_dir()
    cwd = os.getcwd()
    mp.chdir(tmp_path)
    mp.undo()
    assert os.getcwd() == cwd


def test_chdir_double_undo(mp):
    tmp_path = _tmp_dir()
    mp.chdir(str(tmp_path))
    mp.undo()
    os.chdir(tmp_path)
    mp.undo()
    assert os.getcwd() == str(tmp_path)
",
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 4 tests across 1 worker
            PASS [TIME] test::test_chdir_with_path_local(mp=<MockEnv object>)
            PASS [TIME] test::test_chdir_with_str(mp=<MockEnv object>)
            PASS [TIME] test::test_chdir_undo(mp=<MockEnv object>)
            PASS [TIME] test::test_chdir_double_undo(mp=<MockEnv object>)
    ────────────
         Summary [TIME] 4 tests run: 4 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_issue156_undo_staticmethod() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva


class Sample:
    @staticmethod
    def hello():
        return True


class SampleInherit(Sample):
    pass


@karva.tags.parametrize("SampleClass", [Sample, SampleInherit])
def test_issue156_undo_staticmethod(SampleClass):
    monkeypatch = karva.MockEnv()

    monkeypatch.setattr(SampleClass, "hello", None)
    assert SampleClass.hello is None

    monkeypatch.undo()
    assert SampleClass.hello()
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_issue156_undo_staticmethod(SampleClass=<class 'test.Sample'>)
            PASS [TIME] test::test_issue156_undo_staticmethod(SampleClass=<class 'test.SampleInherit'>)
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_undo_class_descriptors_delattr() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva


def test_undo_class_descriptors_delattr():
    class SampleParent:
        @classmethod
        def hello(_cls):
            pass

        @staticmethod
        def world():
            pass

    class SampleChild(SampleParent):
        pass

    monkeypatch = karva.MockEnv()

    original_hello = SampleChild.hello
    original_world = SampleChild.world
    monkeypatch.delattr(SampleParent, "hello")
    monkeypatch.delattr(SampleParent, "world")
    assert getattr(SampleParent, "hello", None) is None
    assert getattr(SampleParent, "world", None) is None

    monkeypatch.undo()
    assert original_hello == SampleChild.hello
    assert original_world == SampleChild.world
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 1 test across 1 worker
            PASS [TIME] test::test_undo_class_descriptors_delattr
    ────────────
         Summary [TIME] 1 test run: 1 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn test_context_manager_forms() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import functools
import inspect

import karva


def test_context():
    monkeypatch = karva.MockEnv()

    with monkeypatch.context() as m:
        m.setattr(functools, "partial", 3)
        assert not inspect.isclass(functools.partial)
    assert inspect.isclass(functools.partial)


def test_context_classmethod():
    class A:
        x = 1

    with karva.MockEnv.context() as m:
        m.setattr(A, "x", 2)
        assert A.x == 2
    assert A.x == 1
"#,
    );

    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 2 tests across 1 worker
            PASS [TIME] test::test_context
            PASS [TIME] test::test_context_classmethod
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
}
