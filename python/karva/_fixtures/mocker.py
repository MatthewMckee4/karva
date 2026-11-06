"""Function-local patch ownership built on Python's standard mock library."""

from __future__ import annotations

import inspect
from typing import Any
from unittest import mock


class _Patcher:
    """Start patches immediately and register them with their owning fixture."""

    def __init__(self, owner: MockerFixture) -> None:
        self._owner = owner

    def __call__(self, *args: Any, **kwargs: Any) -> Any:
        return self._owner._start(mock.patch(*args, **kwargs))

    def object(self, *args: Any, **kwargs: Any) -> Any:
        """Patch an attribute until fixture teardown or an explicit stop."""
        return self._owner._start(mock.patch.object(*args, **kwargs))

    def dict(self, *args: Any, **kwargs: Any) -> Any:
        """Patch a mapping and restore its original contents on teardown."""
        return self._owner._start(mock.patch.dict(*args, **kwargs), mapping=True)

    def multiple(self, *args: Any, **kwargs: Any) -> Any:
        """Patch several attributes, returning mocks requested with DEFAULT."""
        return self._owner._start(mock.patch.multiple(*args, **kwargs), multiple=True)


class MockerFixture:
    """Own patches for one test attempt; direct mock constructors are untracked."""

    Mock = mock.Mock
    MagicMock = mock.MagicMock
    AsyncMock = mock.AsyncMock
    PropertyMock = mock.PropertyMock
    ANY = mock.ANY
    DEFAULT = mock.DEFAULT
    call = mock.call
    sentinel = mock.sentinel
    mock_open = staticmethod(mock.mock_open)

    def __init__(self) -> None:
        self.patch = _Patcher(self)
        self._patches: list[tuple[Any, Any, list[Any]]] = []

    def _start(
        self, patcher: Any, *, mapping: bool = False, multiple: bool = False
    ) -> Any:
        result = patcher.start()
        targets = [] if mapping else list(result.values()) if multiple else [result]
        self._patches.append((result, patcher, targets))
        return result

    def stop(self, mock: Any) -> None:
        """Restore the patch that returned this object, leaving others active."""
        for index in range(len(self._patches) - 1, -1, -1):
            result, patcher, _ = self._patches[index]
            if result is mock:
                patcher.stop()
                del self._patches[index]
                return
        raise ValueError("This object is not an active patch owned by this mocker")

    def stopall(self) -> None:
        """Restore owned patches in reverse order, without touching other owners."""
        errors = []
        while self._patches:
            _, patcher, _ = self._patches.pop()
            try:
                patcher.stop()
            except Exception as error:
                errors.append(error)
        if errors:
            raise ExceptionGroup("Failed to stop mocker patches", errors)

    def resetall(
        self, *, return_value: bool = False, side_effect: bool = False
    ) -> None:
        """Reset tracked call history; optionally reset return values and effects."""
        for _, _, targets in self._patches:
            for result in targets:
                self._reset(result, return_value=return_value, side_effect=side_effect)

    @staticmethod
    def _reset(result: Any, *, return_value: bool, side_effect: bool) -> None:
        if isinstance(result, mock.Mock):
            result.reset_mock(return_value=return_value, side_effect=side_effect)
        elif inspect.isfunction(result) and isinstance(
            getattr(result, "mock", None), mock.Mock
        ):
            # Autospecced functions keep effective settings on their wrapper,
            # whose reset method accepts no keyword flags.
            result.reset_mock()
            result.mock.reset_mock(return_value=return_value, side_effect=side_effect)
            if return_value:
                result.return_value = mock.DEFAULT
            if side_effect:
                result.side_effect = None
        else:
            return
        if "spy_return_list" in vars(result):
            result.spy_return = None
            result.spy_exception = None
            result.spy_return_list.clear()

    def spy(self, obj: Any, name: str) -> Any:
        """Record calls, results and exceptions while running the real callable."""
        original = getattr(obj, name)

        def invoke(*args: Any, **kwargs: Any) -> Any:
            spy.spy_return = None
            spy.spy_exception = None
            try:
                result = original(*args, **kwargs)
            except BaseException as error:
                spy.spy_exception = error
                raise
            spy.spy_return = result
            spy.spy_return_list.append(result)
            return result

        async def async_invoke(*args: Any, **kwargs: Any) -> Any:
            spy.spy_return = None
            spy.spy_exception = None
            try:
                result = await original(*args, **kwargs)
            except BaseException as error:
                spy.spy_exception = error
                raise
            spy.spy_return = result
            spy.spy_return_list.append(result)
            return result

        spy = self.patch.object(
            obj,
            name,
            autospec=inspect.isfunction(original) or inspect.ismethod(original),
            side_effect=async_invoke
            if inspect.iscoroutinefunction(original)
            else invoke,
        )
        spy.spy_return = None
        spy.spy_exception = None
        spy.spy_return_list = []
        return spy

    def stub(self, name: str | None = None) -> mock.MagicMock:
        """Return an untracked callback accepting any arguments."""
        return mock.MagicMock(spec=lambda *_args, **_kwargs: None, name=name)

    def async_stub(self, name: str | None = None) -> mock.AsyncMock:
        """Return an untracked async callback accepting any arguments."""
        return mock.AsyncMock(spec=lambda *_args, **_kwargs: None, name=name)

    def __repr__(self) -> str:
        return "<MockerFixture>"
