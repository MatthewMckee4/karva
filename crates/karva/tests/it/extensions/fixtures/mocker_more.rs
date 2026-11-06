use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[test]
fn mocker_autospec_seal_context_manager_and_reset() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import sys
import karva


def read(key, *, default=None):
    return default


async def fetch(key):
    return key


def test_autospec(mocker):
    callback = mocker.create_autospec(read, return_value=42)
    assert callback("key", default=1) == 42
    callback.assert_called_once_with("key", default=1)
    with karva.raises(TypeError):
        callback()
    mocker.resetall()
    callback.assert_not_called()
    assert callback("key") == 42
    callback.side_effect = ValueError("reset me")
    mocker.resetall(return_value=True, side_effect=True)
    assert callback.side_effect is None
    assert isinstance(callback("key"), mocker.MagicMock)
    mocker.stop(callback)
    callback("key")
    mocker.resetall()
    assert callback.call_count == 2
    mock = mocker.Mock()
    mock.allowed = mocker.Mock(return_value=1)
    mocker.seal(mock)
    assert mock.allowed() == 1
    with karva.raises(AttributeError):
        mock.missing()


async def test_async_autospec(mocker):
    callback = mocker.create_autospec(fetch, return_value=42)
    assert await callback("key") == 42
    callback.assert_awaited_once_with("key")
    with karva.raises(TypeError):
        await callback()
    mocker.resetall()
    callback.assert_not_awaited()


def test_multiple_explicit_and_context_manager(mocker):
    class Service:
        first = None
        second = None
        resource = None
    explicit = mocker.Mock(return_value=42)
    result = mocker.patch.multiple(Service, first=explicit, second=mocker.DEFAULT)
    Service.first()
    Service.second()
    mocker.resetall()
    explicit.assert_not_called()
    result["second"].assert_not_called()
    resource = mocker.patch.context_manager(Service, "resource")
    resource.return_value.__enter__.return_value = "opened"
    with Service.resource() as value:
        assert value == "opened"
    resource.return_value.__exit__.assert_called_once_with(None, None, None)
    mocker.stopall()
    assert (Service.first, Service.second, Service.resource) == (None, None, None)


def test_spy_reset_restores_forwarding(mocker):
    spy = mocker.spy(sys.modules[__name__], "read")
    spy.side_effect = ValueError("override")
    with karva.raises(ValueError, match="override"):
        read("key")
    mocker.resetall(side_effect=True)
    assert read("key", default=42) == 42
    spy.assert_called_once_with("key", default=42)
    assert spy.spy_return == 42


async def test_async_spy_reset(mocker):
    spy = mocker.spy(sys.modules[__name__], "fetch")
    spy.side_effect = ValueError("override")
    with karva.raises(ValueError, match="override"):
        await fetch("key")
    mocker.resetall(return_value=True, side_effect=True)
    assert await fetch("key") == "key"
    spy.assert_awaited_once_with("key")
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 5 tests across 1 worker
            PASS [TIME] test::test_autospec(mocker)
            PASS [TIME] test::test_async_autospec(mocker)
            PASS [TIME] test::test_multiple_explicit_and_context_manager(mocker)
            PASS [TIME] test::test_spy_reset_restores_forwarding(mocker)
            PASS [TIME] test::test_async_spy_reset(mocker)
    ────────────
         Summary [TIME] 5 tests run: 5 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn mocker_spy_duplicates_iterators_without_consuming_them() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import sys
import karva


def values(kind):
    if kind == "error":
        raise ValueError("failed")
    if kind == "list":
        return [1, 2]
    return iter([1, 2])


async def async_values():
    return iter([3, 4])


def test_duplicate(mocker):
    spy = mocker.spy(sys.modules[__name__], "values", duplicate_iterators=True)
    actual = values("iterator")
    assert next(actual) == 1
    assert list(spy.spy_return_iter) == [1, 2]
    assert list(actual) == [2]
    assert spy.spy_return is actual
    assert spy.spy_return_list == [actual]
    assert values("list") == [1, 2]
    assert spy.spy_return_iter is None
    with karva.raises(ValueError):
        values("error")
    assert spy.spy_return_iter is None
    assert isinstance(spy.spy_exception, ValueError)
    values("iterator")
    mocker.resetall(side_effect=True)
    assert spy.spy_return_iter is None
    assert spy.spy_return_list == []
    assert spy.spy_exception is None
    assert list(values("iterator")) == [1, 2]
    assert list(spy.spy_return_iter) == [1, 2]


def test_default_leaves_iterator_alone(mocker):
    spy = mocker.spy(sys.modules[__name__], "values")
    actual = values("iterator")
    assert actual is spy.spy_return
    assert spy.spy_return_iter is None
    assert list(actual) == [1, 2]


async def test_async_returning_iterator(mocker):
    spy = mocker.spy(sys.modules[__name__], "async_values", duplicate_iterators=True)
    actual = await async_values()
    assert list(actual) == [3, 4]
    assert list(spy.spy_return_iter) == [3, 4]
    spy.assert_awaited_once_with()
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 3 tests across 1 worker
            PASS [TIME] test::test_duplicate(mocker)
            PASS [TIME] test::test_default_leaves_iterator_alone(mocker)
            PASS [TIME] test::test_async_returning_iterator(mocker)
    ────────────
         Summary [TIME] 3 tests run: 3 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn scoped_mockers_share_calls_and_restore_at_scope_end(
    #[values("module", "package", "session")] scope: &str,
) {
    let context = TestContext::with_files([
        ("shared.py", "value = 'original'\n".to_owned()),
        (
            "test.py",
            r#"
import os
from pathlib import Path
import karva
import shared


@karva.fixture(scope="session")
def observer():
    yield
    assert shared.value == "original"
    Path("restored.txt").write_text("restored", encoding="utf-8")


def test_first(observer, SCOPED):
    attempt = int(os.environ["KARVA_ATTEMPT"])
    if attempt == 1:
        assert shared.value == "original"
        SCOPED.patch("shared.value", return_value=42)
    assert shared.value() == 42
    assert shared.value.call_count == attempt
    assert attempt == 2


def test_second(SCOPED):
    assert shared.value.call_count == 2
    assert shared.value() == 42
    assert shared.value.call_count == 3
"#
            .replace("SCOPED", &format!("{scope}_mocker")),
        ),
    ]);
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(r"(?:module|package|session)_mocker", "scoped_mocker");
    let _guard = settings.bind_to_scope();
    // The retry deliberately proves that a broader-scoped mock retains its
    // call history, while the session observer proves final cleanup occurred.
    insta::allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel().args([
            "--retry=1", "--status-level=none"
        ]), @"
        success: true
        exit_code: 0
        ----- stdout -----
        ────────────
             Summary [TIME] 2 tests run: 2 passed (1 flaky), 0 skipped
           FLAKY 2/2 [TIME] test::test_first(observer=None, scoped_mocker)

        ----- stderr -----
        ");
    }
    assert_eq!(context.read_file("restored.txt"), "restored");
}

#[rstest]
fn scoped_mockers_respect_module_and_package_boundaries(
    #[values("module", "package", "session")] scope: &str,
) {
    let context = TestContext::with_files([
        (
            "shared.py",
            r#"
value = "original"
owner = None
calls = 0


def exercise(mocker, scope, module, package):
    global owner, calls
    key = module if scope == "module" else package if scope == "package" else "session"
    if owner != key:
        assert value == "original"
        mocker.patch("shared.value", return_value=42)
        owner = key
        calls = 0
    assert value() == 42
    calls += 1
    assert value.call_count == calls
"#
            .to_owned(),
        ),
        (
            "conftest.py",
            r#"
from pathlib import Path
import karva
import shared


@karva.fixture(scope="session", auto_use=True)
def observer():
    yield
    assert shared.value == "original"
    Path("restored.txt").write_text("restored", encoding="utf-8")
"#
            .to_owned(),
        ),
        ("a/__init__.py", String::new()),
        ("b/__init__.py", String::new()),
        (
            "a/test_first.py",
            format!(
                r#"
import shared


def test_first({scope}_mocker):
    shared.exercise({scope}_mocker, "{scope}", "a.first", "a")
"#
            ),
        ),
        (
            "a/test_second.py",
            format!(
                r#"
import shared


def test_second({scope}_mocker):
    shared.exercise({scope}_mocker, "{scope}", "a.second", "a")
"#
            ),
        ),
        (
            "b/test_third.py",
            format!(
                r#"
import shared


def test_third({scope}_mocker):
    shared.exercise({scope}_mocker, "{scope}", "b.third", "b")
"#
            ),
        ),
    ]);
    insta::allow_duplicates! {
        assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
        success: true
        exit_code: 0
        ----- stdout -----
        ────────────
             Summary [TIME] 3 tests run: 3 passed, 0 skipped

        ----- stderr -----
        ");
    }
    assert_eq!(context.read_file("restored.txt"), "restored");
}

#[test]
fn package_mocker_dependency_lives_as_long_as_its_parent_fixture() {
    let context = TestContext::with_files([
        ("shared.py", "value = 'original'\nparent = None\n"),
        (
            "conftest.py",
            r#"
from pathlib import Path
import karva
import shared


@karva.fixture(scope="session", auto_use=True)
def observer():
    yield
    assert shared.value == "original"
    Path("restored.txt").write_text("restored", encoding="utf-8")


@karva.fixture(scope="package")
def parent(package_mocker):
    assert shared.value == "original"
    shared.parent = package_mocker
    return package_mocker.patch("shared.value", return_value=42)
"#,
        ),
        ("a/__init__.py", ""),
        ("b/__init__.py", ""),
        (
            "a/test_first.py",
            r"
import shared


def test_first(package_mocker, parent):
    assert package_mocker is not shared.parent
    assert shared.value is parent
    assert shared.value() == 42
",
        ),
        (
            "b/test_second.py",
            r"
import shared


def test_second(parent, package_mocker):
    assert package_mocker is not shared.parent
    assert shared.value is parent
    assert shared.value() == 42
",
        ),
    ]);
    assert_cmd_snapshot!(context.command_no_parallel().arg("--status-level=none"), @"
    success: true
    exit_code: 0
    ----- stdout -----
    ────────────
         Summary [TIME] 2 tests run: 2 passed, 0 skipped

    ----- stderr -----
    ");
    assert_eq!(context.read_file("restored.txt"), "restored");
}
