use insta_cmd::assert_cmd_snapshot;
use rstest::rstest;

use crate::common::TestContext;

#[test]
fn mocker_patches_and_resets_owned_mocks() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from unittest import mock
import karva


def test_patch(mocker: karva.MockerFixture):
    for name in ("Mock", "MagicMock", "AsyncMock", "PropertyMock", "ANY", "call", "sentinel", "mock_open"):
        assert getattr(mocker, name) == getattr(mock, name)
    original = os.getcwd
    patched = mocker.patch("os.getcwd", return_value="fake")
    assert os.getcwd() == "fake"
    patched.assert_called_once_with()
    assert patched.mock_calls == [mocker.call()]
    mocker.resetall()
    patched.assert_not_called()
    assert os.getcwd() == "fake"
    mocker.stop(patched)
    assert os.getcwd is original


def test_variants(mocker):
    class Service:
        def read(self, key):
            return key

        @property
        def value(self):
            return 1

    patched = mocker.patch.object(Service, "read", autospec=True, return_value=42)
    service = Service()
    assert service.read("key") == 42
    patched.assert_called_once_with(service, mocker.ANY)
    prop = mocker.patch.object(Service, "value", new_callable=mocker.PropertyMock, return_value=9)
    assert service.value == 9
    mocker.resetall()
    patched.assert_not_called()
    prop.assert_not_called()
    patched.side_effect = lambda *args: 99
    assert service.read("key") == 99
    mocker.resetall(return_value=True, side_effect=True)
    patched.assert_not_called()
    assert patched.side_effect is None
    assert isinstance(service.read("key"), mocker.MagicMock)
    mocker.stopall()
    assert service.read("key") == "key"
    assert service.value == 1
    mocker.stopall()


def test_mapping_and_multiple(mocker):
    class Service:
        first = 1
        second = 2

    unowned = mocker.Mock()
    unowned()
    mapping = {"unowned": unowned}
    result = mocker.patch.dict(mapping, {"new": 2})
    assert result is mapping
    mocks = mocker.patch.multiple(Service, first=mocker.DEFAULT, second=mocker.DEFAULT)
    Service.first(1)
    Service.second(2)
    mocker.resetall()
    mocks["first"].assert_not_called()
    mocks["second"].assert_not_called()
    unowned.assert_called_once_with()
    mocker.stop(mocks)
    assert (Service.first, Service.second) == (1, 2)
    mocker.stop(result)
    assert mapping == {"unowned": unowned}


def test_reset_options_and_ownership(mocker):
    first = mocker.patch("os.getcwd", return_value="outer")
    other = karva.MockerFixture()
    try:
        second = other.patch("os.getcwd", return_value="inner")
        other.stopall()
        assert os.getcwd() == "outer"
        second.assert_not_called()
    finally:
        other.stopall()
    first.side_effect = ValueError("failure")
    mocker.resetall(return_value=True, side_effect=True)
    assert first.side_effect is None
    assert first.return_value != "outer"
    mocker.stopall()
    with karva.raises(ValueError, match="not an active patch"):
        mocker.stop(first)


def test_stopall_continues_after_restore_error(mocker):
    class Meta(type):
        def __setattr__(cls, name, value):
            if name == "value" and value == "original":
                raise RuntimeError("cannot restore")
            super().__setattr__(name, value)

    class Service(metaclass=Meta):
        value = "original"

    original = os.getcwd
    mocker.patch("os.getcwd", return_value="patched")
    mocker.patch.object(Service, "value", "patched")
    with karva.raises(ExceptionGroup, match="Failed to stop mocker patches") as error:
        mocker.stopall()
    assert len(error.value.exceptions) == 1
    assert str(error.value.exceptions[0]) == "cannot restore"
    assert os.getcwd is original
    mocker.stopall()

"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 5 tests across 1 worker
            PASS [TIME] test::test_patch(mocker)
            PASS [TIME] test::test_variants(mocker)
            PASS [TIME] test::test_mapping_and_multiple(mocker)
            PASS [TIME] test::test_reset_options_and_ownership(mocker)
            PASS [TIME] test::test_stopall_continues_after_restore_error(mocker)
    ────────────
         Summary [TIME] 5 tests run: 5 passed, 0 skipped

    ----- stderr -----
    ");
}

#[test]
fn mocker_spies_and_stubs_sync_and_async_callables() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import sys
import karva


def compute(value):
    if value < 0:
        raise ValueError("negative")
    return value * 2


async def fetch(value):
    return compute(value)


def test_spy(mocker):
    spy = mocker.spy(sys.modules[__name__], "compute")
    assert compute(2) == 4
    assert compute(3) == 6
    spy.assert_any_call(2)
    spy.assert_has_calls([mocker.call(2), mocker.call(3)])
    assert spy.spy_return == 6
    assert spy.spy_return_list == [4, 6]
    with karva.raises(ValueError, match="negative") as error:
        compute(-1)
    assert spy.spy_exception is error.value
    assert spy.spy_return is None
    assert compute(4) == 8
    assert spy.spy_exception is None
    mocker.resetall()
    spy.assert_not_called()
    assert spy.spy_return is None
    assert spy.spy_return_list == []
    mocker.stop(spy)
    assert compute(5) == 10
    spy.assert_not_called()


def test_methods(mocker):
    class Service:
        def method(self, value):
            return value * 2

        @classmethod
        def cls_method(cls, value):
            return cls, value

        @staticmethod
        def static(value):
            return value

    service = Service()
    bound = mocker.spy(service, "method")
    assert service.method(3) == 6
    bound.assert_called_once_with(3)
    mocker.stop(bound)
    unbound = mocker.spy(Service, "method")
    assert service.method(4) == 8
    unbound.assert_called_once_with(service, 4)
    cls = mocker.spy(Service, "cls_method")
    assert service.cls_method(5) == (Service, 5)
    cls.assert_called_once_with(5)
    static = mocker.spy(Service, "static")
    assert service.static(6) == 6
    static.assert_called_once_with(6)


async def test_async(mocker):
    spy = mocker.spy(sys.modules[__name__], "fetch")
    assert await fetch(3) == 6
    spy.assert_awaited_once_with(3)
    assert spy.spy_return == 6
    with karva.raises(ValueError, match="negative") as error:
        await fetch(-1)
    assert spy.spy_exception is error.value
    mocker.resetall()
    spy.assert_not_awaited()
    assert spy.spy_exception is None
    patched = mocker.patch(__name__ + ".fetch", return_value=42)
    assert await fetch(7) == 42
    patched.assert_awaited_once_with(7)
    stub = mocker.async_stub(name="callback")
    await stub(1, key=2)
    stub.assert_awaited_once_with(1, key=2)
    mocker.resetall()
    stub.assert_awaited_once_with(1, key=2)
    mocker.stopall()
    patched = mocker.patch(__name__ + ".fetch", autospec=True, return_value=42,
                           side_effect=lambda value: 99)
    assert await fetch(7) == 99
    mocker.resetall(return_value=True, side_effect=True)
    patched.assert_not_awaited()
    assert patched.side_effect is None
    assert isinstance(await fetch(7), mocker.AsyncMock)


def test_stub(mocker):
    stub = mocker.stub(name="callback")
    stub(1, key=2)
    stub.assert_called_once_with(1, key=2)
    mocker.resetall()
    stub.assert_called_once_with(1, key=2)
"#,
    );
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: true
    exit_code: 0
    ----- stdout -----
        Starting 4 tests across 1 worker
            PASS [TIME] test::test_spy(mocker)
            PASS [TIME] test::test_methods(mocker)
            PASS [TIME] test::test_async(mocker)
            PASS [TIME] test::test_stub(mocker)
    ────────────
         Summary [TIME] 4 tests run: 4 passed, 0 skipped

    ----- stderr -----
    ");
}

#[rstest]
fn mocker_is_fresh_for_cases_and_retries(#[values(1, 2)] workers: usize) {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path
import karva


@karva.tags.parametrize("case", [1, 2])
def test_retry(mocker, case):
    assert "KARVA_MOCKER_VALUE" not in os.environ
    mocker.patch.dict(os.environ, {"KARVA_MOCKER_VALUE": str(case)})
    assert os.environ["KARVA_MOCKER_VALUE"] == str(case)
    assert os.environ["KARVA_ATTEMPT"] == "2"
    Path(f"case-{case}.txt").write_text("retried", encoding="utf-8")
"#,
    );
    let mut command = context.command();
    command
        .arg(format!("--num-workers={workers}"))
        .arg("--retry=1");
    // Worker completion order is intentionally unspecified.
    command.args(["--status-level=none", "--final-status-level=none"]);
    assert_cmd_snapshot!(command, @"
    success: true
    exit_code: 0
    ----- stdout -----

    ----- stderr -----
    ");
    assert_eq!(context.read_file("case-1.txt"), "retried");
    assert_eq!(context.read_file("case-2.txt"), "retried");
}

#[rstest]
fn mocker_restores_patches_after_failures(#[values("test", "setup", "teardown")] phase: &str) {
    let context = TestContext::with_file(
        "test.py",
        r#"
import os
from pathlib import Path
import karva


class Service:
    value = "original"


@karva.fixture(scope="module")
def observer():
    yield
    assert Service.value == "original"
    assert "KARVA_MOCKER_VALUE" not in os.environ
    Path("restored.txt").write_text("restored", encoding="utf-8")


@karva.fixture
def resource(observer, mocker):
    mocker.patch.object(Service, "value", "patched")
    mocker.patch.dict(os.environ, {"KARVA_MOCKER_VALUE": "patched"})
    if os.environ["FAIL_PHASE"] == "setup":
        raise RuntimeError("setup failed")
    yield
    if os.environ["FAIL_PHASE"] == "teardown":
        raise RuntimeError("teardown failed")


def test_failure(resource):
    if os.environ["FAIL_PHASE"] == "test":
        raise RuntimeError("test failed")


def test_restored():
    assert Service.value == "original"
    assert "KARVA_MOCKER_VALUE" not in os.environ
"#,
    );
    match phase {
        "test" => assert_cmd_snapshot!(context.command_no_parallel().env("FAIL_PHASE", phase), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 2 tests across 1 worker
                FAIL [TIME] test::test_failure(resource=None)
                PASS [TIME] test::test_restored

        failures:

        test::test_failure(resource=None):

        error[test-failure]: Test `test_failure` failed
          --> test.py:30:5
           |
        30 | def test_failure(resource):
           |     ^^^^^^^^^^^^
        info: Test ran with arguments:
        info:   `resource`: `None`
        info: Test failed here
          --> test.py:32:9
           |
        32 |         raise RuntimeError("test failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: test failed

        ────────────
             Summary [TIME] 2 tests run: 1 passed, 1 failed, 0 skipped

        ----- stderr -----
        "#),
        "setup" => {
            assert_cmd_snapshot!(context.command_no_parallel().env("FAIL_PHASE", phase), @r#"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 2 tests across 1 worker
               ERROR [TIME] test::test_failure
                PASS [TIME] test::test_restored

        failures:

        test::test_failure (requires fixture `resource`):

        error[fixture-failure]: Fixture `resource` failed
          --> test.py:20:5
           |
        20 | def resource(observer, mocker):
           |     ^^^^^^^^
        info: Fixture ran with arguments:
        info:   `observer`: `None`
        info:   `mocker`: `<MockerFixture>`
        info: Fixture failed here
          --> test.py:24:9
           |
        24 |         raise RuntimeError("setup failed")
           |         ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
        info: setup failed

        ────────────
             Summary [TIME] 2 tests run: 1 passed, 1 error, 0 skipped

        ----- stderr -----
        "#)
        }
        _ => assert_cmd_snapshot!(context.command_no_parallel().env("FAIL_PHASE", phase), @"
        success: false
        exit_code: 1
        ----- stdout -----
            Starting 2 tests across 1 worker
               ERROR [TIME] test::test_failure(resource=None)
                PASS [TIME] test::test_restored

        failures:

        test::test_failure(resource=None):

        error[invalid-fixture-finalizer]: Discovered an invalid fixture finalizer `resource`
          --> test.py:20:5
           |
        20 | def resource(observer, mocker):
           |     ^^^^^^^^
        info: Failed to reset fixture: teardown failed

        ────────────
             Summary [TIME] 2 tests run: 1 passed, 1 error, 0 skipped

        ----- stderr -----
        "),
    }
    assert_eq!(context.read_file("restored.txt"), "restored");
}

#[test]
fn mocker_assertion_helpers_report_wrong_calls() {
    let context = TestContext::with_file(
        "test.py",
        r#"
import karva


def test_wrong_call(mocker):
    callback = mocker.stub()
    callback("actual")
    with karva.raises(AssertionError, match="expected call not found") as error:
        callback.assert_called_once_with("expected")
    assert "Expected: mock('expected')" in str(error.value)
    assert "Actual: mock('actual')" in str(error.value)
    callback.assert_not_called()
"#,
    );
    // Library source paths and line numbers differ across Python installations.
    let mut settings = insta::Settings::clone_current();
    settings.add_filter(
        r"(?s)info: Test failed here\n.*?\ninfo: ",
        "info: Test failed in unittest.mock\ninfo: ",
    );
    let _guard = settings.bind_to_scope();
    assert_cmd_snapshot!(context.command_no_parallel(), @"
    success: false
    exit_code: 1
    ----- stdout -----
        Starting 1 test across 1 worker
            FAIL [TIME] test::test_wrong_call(mocker)

    failures:

    test::test_wrong_call(mocker):

    error[test-failure]: Test `test_wrong_call` failed
     --> test.py:5:5
      |
    5 | def test_wrong_call(mocker):
      |     ^^^^^^^^^^^^^^^
    info: Test ran with arguments:
    info:   `mocker`: `<MockerFixture>`
    info: Test failed in unittest.mock
    info: Expected 'mock' to not have been called. Called 1 times.
          Calls: [call('actual')].

    ────────────
         Summary [TIME] 1 test run: 0 passed, 1 failed, 0 skipped

    ----- stderr -----
    ");
}
