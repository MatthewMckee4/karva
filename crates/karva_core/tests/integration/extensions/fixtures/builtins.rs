use insta::{allow_duplicates, assert_snapshot};
use karva_test::TestContext;
use rstest::rstest;

use crate::common::TestRunnerExt;

#[rstest]
fn test_temp_directory_fixture(
    #[values("tmp_path", "temp_path", "temp_dir", "tmpdir")] fixture_name: &str,
) {
    let test_context = TestContext::with_file(
        "<test>/test.py",
        &format!(
            r"
                import pathlib

                def test_temp_directory_fixture({fixture_name}):
                    assert {fixture_name}.exists()
                    assert {fixture_name}.is_dir()
                    assert {fixture_name}.is_absolute()
                    assert isinstance({fixture_name}, pathlib.Path)
                "
        ),
    );

    let result = test_context.test();

    allow_duplicates! {
        assert_snapshot!(result.display(), @"test result: ok. 1 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}

#[test]
fn test_monkeypatch_setattr() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
from karva import Mock

def test_setattr_simple(monkeypatch):
    class A:
        x = 1

    monkeypatch.setattr(A, 'x', 2)
    assert A.x == 2

def test_setattr_new_attribute(monkeypatch):
    class A:
        x = 1

    monkeypatch.setattr(A, 'y', 2, raising=False)
    assert A.y == 2

def test_setattr_undo(monkeypatch):
    class A:
        x = 1

    monkeypatch.setattr(A, 'x', 2)
    assert A.x == 2
    monkeypatch.undo()
    assert A.x == 1
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_monkeypatch_setitem() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
def test_setitem_dict(monkeypatch):
    d = {'x': 1}
    monkeypatch.setitem(d, 'x', 2)
    assert d['x'] == 2

def test_setitem_new_key(monkeypatch):
    d = {'x': 1}
    monkeypatch.setitem(d, 'y', 2)
    assert d['y'] == 2
    monkeypatch.undo()
    assert 'y' not in d

def test_setitem_undo(monkeypatch):
    d = {'x': 1}
    monkeypatch.setitem(d, 'x', 2)
    monkeypatch.undo()
    assert d['x'] == 1
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_monkeypatch_env() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import os

def test_setenv(monkeypatch):
    monkeypatch.setenv('TEST_VAR', 'test_value')
    assert os.environ['TEST_VAR'] == 'test_value'

def test_setenv_undo(monkeypatch):
    monkeypatch.setenv('TEST_VAR_2', 'test_value')
    assert os.environ['TEST_VAR_2'] == 'test_value'
    monkeypatch.undo()
    assert 'TEST_VAR_2' not in os.environ

def test_delenv(monkeypatch):
    os.environ['TEST_VAR_3'] = 'value'
    monkeypatch.delenv('TEST_VAR_3')
    assert 'TEST_VAR_3' not in os.environ
    monkeypatch.undo()
    assert os.environ['TEST_VAR_3'] == 'value'
    del os.environ['TEST_VAR_3']
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_monkeypatch_syspath() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import sys

def test_syspath_prepend(monkeypatch):
    old_path = sys.path.copy()
    monkeypatch.syspath_prepend('/test/path')
    assert sys.path[0] == '/test/path'
    monkeypatch.undo()
    assert sys.path == old_path

def test_syspath_prepend_multiple(monkeypatch):
    old_path = sys.path.copy()
    monkeypatch.syspath_prepend('/first')
    monkeypatch.syspath_prepend('/second')
    assert sys.path[0] == '/second'
    assert sys.path[1] == '/first'
    monkeypatch.undo()
    assert sys.path == old_path
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_monkeypatch_delattr() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
def test_delattr(monkeypatch):
    class A:
        x = 1

    monkeypatch.delattr(A, 'x')
    assert not hasattr(A, 'x')

def test_delattr_undo(monkeypatch):
    class A:
        x = 1

    monkeypatch.delattr(A, 'x')
    assert not hasattr(A, 'x')
    monkeypatch.undo()
    assert A.x == 1
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_monkeypatch_context_manager() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
from karva import Mock

def test_context_manager():
    class A:
        x = 1

    with Mock() as m:
        m.setattr(A, 'x', 2)
        assert A.x == 2

    assert A.x == 1

def test_context_manager_auto_undo():
    d = {'x': 1}

    with Mock() as m:
        m.setitem(d, 'x', 2)
        assert d['x'] == 2

    assert d['x'] == 1
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

#[test]
fn test_finalizer() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r"
import os

def test_setenv(monkeypatch):
    monkeypatch.setenv('TEST_VAR_4', 'test_value')
    assert os.environ['TEST_VAR_4'] == 'test_value'

def test_1():
    assert 'TEST_VAR_4' not in os.environ
        ",
    );

    let result = context.test();

    assert_snapshot!(result.display(), @"test result: ok. 2 passed; 0 failed; 0 skipped; finished in [TIME]");
}

/// Taken from <https://github.com/pytest-dev/pytest/blob/main/testing/test_monkeypatch.py>
#[test]
fn test_mock() {
    let context = TestContext::with_file(
        "<test>/test.py",
        r#"
            import os
            import re
            import sys
            from collections.abc import Generator
            from pathlib import Path

            import karva
            import pytest
            from karva import Mock

            skip_macos = karva.tags.skip(sys.platform == "darwin")

            @karva.fixture
            def mp() -> Generator[Mock]:
                cwd = os.getcwd()
                sys_path = list(sys.path)
                yield Mock()
                sys.path[:] = sys_path
                os.chdir(cwd)


            def test_setattr() -> None:
                class A:
                    x = 1

                monkeypatch = Mock()
                pytest.raises(AttributeError, monkeypatch.setattr, A, "notexists", 2)
                monkeypatch.setattr(A, "y", 2, raising=False)
                assert A.y == 2  # ty: ignore
                monkeypatch.undo()
                assert not hasattr(A, "y")

                monkeypatch = Mock()
                monkeypatch.setattr(A, "x", 2)
                assert A.x == 2
                monkeypatch.setattr(A, "x", 3)
                assert A.x == 3
                monkeypatch.undo()
                assert A.x == 1

                A.x = 5
                monkeypatch.undo()  # double-undo makes no modification
                assert A.x == 5

                with pytest.raises(TypeError):
                    monkeypatch.setattr(A, "y")  # type: ignore[call-overload]


            def test_delattr() -> None:
                class A:
                    x = 1

                monkeypatch = Mock()
                monkeypatch.delattr(A, "x")
                assert not hasattr(A, "x")
                monkeypatch.undo()
                assert A.x == 1

                monkeypatch = Mock()
                monkeypatch.delattr(A, "x")
                pytest.raises(AttributeError, monkeypatch.delattr, A, "y")
                monkeypatch.delattr(A, "y", raising=False)
                monkeypatch.setattr(A, "x", 5, raising=False)
                assert A.x == 5
                monkeypatch.undo()
                assert A.x == 1


            def test_setitem() -> None:
                d = {"x": 1}
                monkeypatch = Mock()
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


            def test_setitem_deleted_meanwhile() -> None:
                d: dict[str, object] = {}
                monkeypatch = Mock()
                monkeypatch.setitem(d, "x", 2)
                del d["x"]
                monkeypatch.undo()
                assert not d


            @pytest.mark.parametrize("before", [True, False])
            def test_setenv_deleted_meanwhile(before: bool) -> None:
                key = "qwpeoip123"
                if before:
                    os.environ[key] = "world"
                monkeypatch = Mock()
                monkeypatch.setenv(key, "hello")
                del os.environ[key]
                monkeypatch.undo()
                if before:
                    assert os.environ[key] == "world"
                    del os.environ[key]
                else:
                    assert key not in os.environ


            def test_delitem() -> None:
                d: dict[str, object] = {"x": 1}
                monkeypatch = Mock()
                monkeypatch.delitem(d, "x")
                assert "x" not in d
                monkeypatch.delitem(d, "y", raising=False)
                pytest.raises(KeyError, monkeypatch.delitem, d, "y")
                assert not d
                monkeypatch.setitem(d, "y", 1700)
                assert d["y"] == 1700
                d["hello"] = "world"
                monkeypatch.setitem(d, "x", 1500)
                assert d["x"] == 1500
                monkeypatch.undo()
                assert d == {"hello": "world", "x": 1}


            def test_setenv() -> None:
                monkeypatch = Mock()
                monkeypatch.setenv("XYZ123", 2)  # type: ignore[arg-type]
                import os

                assert os.environ["XYZ123"] == "2"
                monkeypatch.undo()
                assert "XYZ123" not in os.environ


            def test_delenv() -> None:
                name = "xyz1234"
                assert name not in os.environ
                monkeypatch = Mock()
                pytest.raises(KeyError, monkeypatch.delenv, name, raising=True)
                monkeypatch.delenv(name, raising=False)
                monkeypatch.undo()
                os.environ[name] = "1"
                try:
                    monkeypatch = Mock()
                    monkeypatch.delenv(name)
                    assert name not in os.environ
                    monkeypatch.setenv(name, "3")
                    assert os.environ[name] == "3"
                    monkeypatch.undo()
                    assert os.environ[name] == "1"
                finally:
                    if name in os.environ:
                        del os.environ[name]

            def test_setenv_prepend() -> None:
                import os

                monkeypatch = Mock()
                monkeypatch.setenv("XYZ123", "2", prepend="-")
                monkeypatch.setenv("XYZ123", "3", prepend="-")
                assert os.environ["XYZ123"] == "3-2"
                monkeypatch.undo()
                assert "XYZ123" not in os.environ


            def test_syspath_prepend(mp: Mock) -> None:
                old = list(sys.path)
                mp.syspath_prepend("world")
                mp.syspath_prepend("hello")
                assert sys.path[0] == "hello"
                assert sys.path[1] == "world"
                mp.undo()
                assert sys.path == old
                mp.undo()
                assert sys.path == old


            def test_syspath_prepend_double_undo(mp: Mock) -> None:
                old_syspath = sys.path[:]
                try:
                    mp.syspath_prepend("hello world")
                    mp.undo()
                    sys.path.append("more hello world")
                    mp.undo()
                    assert sys.path[-1] == "more hello world"
                finally:
                    sys.path[:] = old_syspath


            @skip_macos
            def test_chdir_with_path_local(mp: Mock, tmp_path: Path) -> None:
                mp.chdir(tmp_path)
                assert os.getcwd() == str(tmp_path), f"Expected {str(tmp_path)}, got {os.getcwd()}"

            @skip_macos
            def test_chdir_with_str(mp: Mock, tmp_path: Path) -> None:
                mp.chdir(str(tmp_path))
                assert os.getcwd() == str(tmp_path), f"Expected {str(tmp_path)}, got {os.getcwd()}"


            def test_chdir_undo(mp: Mock, tmp_path: Path) -> None:
                cwd = os.getcwd()
                mp.chdir(tmp_path)
                mp.undo()
                assert os.getcwd() == cwd


            @skip_macos
            def test_chdir_double_undo(mp: Mock, tmp_path: Path) -> None:
                mp.chdir(str(tmp_path))
                mp.undo()
                os.chdir(tmp_path)
                mp.undo()
                assert os.getcwd() == str(tmp_path), f"Expected {str(tmp_path)}, got {os.getcwd()}"
                "#,
    );

    let result = context.test();

    if cfg!(target_os = "macos") {
        assert_snapshot!(result.display(), @"test result: ok. 13 passed; 0 failed; 3 skipped; finished in [TIME]");
    } else {
        assert_snapshot!(result.display(), @"test result: ok. 16 passed; 0 failed; 0 skipped; finished in [TIME]");
    }
}
