"""Scoped patch ownership built on Python's standard mock library."""

from __future__ import annotations

import builtins
import inspect
import os
from collections.abc import Iterator
from contextlib import AbstractContextManager, suppress
from itertools import tee
from pkgutil import resolve_name
from typing import Any, ClassVar
from unittest import mock


class _PatchLayer:
    """Defer restoration while a newer patch covers the same target."""

    _active: ClassVar[dict[tuple[int, str | None], list[_PatchLayer]]] = {}
    _starting: ClassVar[set[tuple[int, str | None]]] = set()

    def __init__(
        self, key: tuple[int, str | None], patcher: AbstractContextManager[Any]
    ) -> None:
        self.key = key
        self.patcher = patcher
        self.stopped = False
        self.mapping_owner: MockerFixture | None = None
        self.uppercase_keys = False

    def start(self) -> Any:
        result = self.patcher.__enter__()
        self.register()
        return result

    def register(self) -> None:
        self._active.setdefault(self.key, []).append(self)

    def stop(self) -> None:
        layers = self._active[self.key]
        if self.key[1] is None and layers[-1] is not self:
            index = layers.index(self)
            self._restore_buried_mapping(self.patcher, layers[index + 1 :])
            del layers[index]
            return

        self.stopped = True
        errors = []
        while layers and layers[-1].stopped:
            layer = layers.pop()
            if not layers:
                del self._active[self.key]
            try:
                layer.patcher.__exit__(None, None, None)
            except Exception as error:
                errors.append(error)
        _raise_cleanup_errors(errors)

    def _restore_buried_mapping(self, patch: Any, newer: list[_PatchLayer]) -> None:
        # patch.dict snapshots the entire mapping. Rebase newer snapshots so
        # their teardown cannot restore retired keys.
        keys = self._mapping_keys(patch)
        if patch.clear:
            keys.update(patch._original)
        for layer in newer:
            successor: Any = layer.patcher
            original = successor._original
            self._restore_mapping_keys(patch, original, keys)
            if successor.clear:
                return
            keys.difference_update(layer._mapping_keys(successor))
        self._restore_mapping_keys(patch, patch.in_dict, keys)

    def _mapping_keys(self, patch: Any) -> set[Any]:
        if self.uppercase_keys:
            return {name.upper() for name in patch.values}
        return set(patch.values)

    @staticmethod
    def _restore_mapping_keys(patch: Any, mapping: Any, keys: set[Any]) -> None:
        if patch.clear and type(mapping) is builtins.dict:
            # A clear removed the original insertion positions. Rebuild those
            # positions in snapshot order, keeping any newer values in place.
            restored = {
                key: patch._original[key] if key in keys else mapping[key]
                for key in patch._original
                if key in keys or key in mapping
            }
            restored.update(
                (key, value)
                for key, value in mapping.items()
                if key not in restored and key not in keys
            )
            mapping.clear()
            mapping.update(restored)
            return
        for key in keys:
            if key in patch._original:
                mapping[key] = patch._original[key]
            else:
                with suppress(KeyError):
                    del mapping[key]


def _raise_cleanup_errors(errors: list[Exception]) -> None:
    if len(errors) == 1:
        raise errors[0]
    if errors:
        raise ExceptionGroup("Failed to stop mocker patches", errors)


def _check_can_stop(layers: list[_PatchLayer]) -> None:
    if any(layer.key in _PatchLayer._starting for layer in layers):
        raise ValueError("Cannot stop a mapping patch while another patch is starting")


def _stop_layers(layers: list[_PatchLayer]) -> None:
    errors = []
    for layer in reversed(layers):
        try:
            layer.stop()
        except Exception as error:
            errors.append(error)
    _raise_cleanup_errors(errors)


class _Patcher:
    """Start patches immediately and register them with their owning fixture."""

    def __init__(self, owner: MockerFixture) -> None:
        self._owner = owner

    def __call__(self, target: str, *args: Any, **kwargs: Any) -> Any:
        patcher = mock.patch(target, *args, **kwargs)
        # unittest.mock reads the original value through the getter too.
        # Pin both lookups to the same object for dynamic dotted targets.
        resolved = patcher.getter()
        patcher.getter = lambda: resolved
        return self._start_attribute(patcher)

    def object(self, target: Any, attribute: str, *args: Any, **kwargs: Any) -> Any:
        """Patch an attribute until fixture teardown or an explicit stop."""
        patcher = mock.patch.object(target, attribute, *args, **kwargs)
        return self._start_attribute(patcher)

    def _start_attribute(self, patcher: Any) -> Any:
        result = patcher.__enter__()
        # Use the exact object resolved by unittest.mock, including for a
        # dotted path with dynamic attribute lookup.
        layer = _PatchLayer((id(patcher.target), patcher.attribute), patcher)
        layer.register()
        self._owner._patches.append((result, [layer], [result]))
        return result

    context_manager = object

    def dict(self, in_dict: Any, *args: Any, **kwargs: Any) -> Any:
        """Patch a mapping and restore its original contents on teardown."""
        if isinstance(in_dict, str):
            in_dict = resolve_name(in_dict)
        key = (id(in_dict), None)
        active = _PatchLayer._active.get(key, [])
        is_environment = in_dict is os.environ
        if key in _PatchLayer._starting:
            raise ValueError("Cannot patch a mapping while another patch is starting")
        if (
            type(in_dict) is not builtins.dict
            and not is_environment
            and any(layer.mapping_owner is not self._owner for layer in active)
        ):
            raise ValueError(
                "Cannot patch this mapping with multiple mockers. "
                "Use one mocker for mappings with custom key behavior."
            )

        _PatchLayer._starting.add(key)
        try:
            patcher = mock.patch.dict(in_dict, *args, **kwargs)
            layer = _PatchLayer(key, patcher)
            layer.mapping_owner = self._owner
            layer.uppercase_keys = is_environment and os.name == "nt"
            result = layer.start()
        finally:
            _PatchLayer._starting.remove(key)
        self._owner._patches.append((result, [layer], []))
        return result

    def multiple(
        self,
        target: Any,
        spec: Any = None,
        create: bool = False,
        spec_set: Any = None,
        autospec: Any = None,
        new_callable: Any = None,
        **kwargs: Any,
    ) -> builtins.dict[str, Any]:
        """Patch several attributes, returning mocks requested with DEFAULT."""
        if not kwargs:
            raise ValueError(
                "Must supply at least one keyword argument with patch.multiple"
            )
        if isinstance(target, str):
            target = resolve_name(target)
        layers = []
        results = {}
        targets = []
        try:
            for attribute, replacement in kwargs.items():
                patcher = mock.patch.object(
                    target,
                    attribute,
                    replacement,
                    spec,
                    create,
                    spec_set,
                    autospec,
                    new_callable,
                )
                layer = _PatchLayer((id(target), attribute), patcher)
                result = layer.start()
                layers.append(layer)
                targets.append(result)
                if replacement is mock.DEFAULT:
                    results[attribute] = result
        except BaseException:
            _stop_layers(layers)
            raise
        self._owner._patches.append((results, layers, targets))
        return results


class MockerFixture:
    """Own patches for one fixture lifetime; direct mock constructors are untracked."""

    Mock = mock.Mock
    MagicMock = mock.MagicMock
    AsyncMock = mock.AsyncMock
    PropertyMock = mock.PropertyMock
    ANY = mock.ANY
    DEFAULT = mock.DEFAULT
    call = mock.call
    sentinel = mock.sentinel
    mock_open = staticmethod(mock.mock_open)
    seal = staticmethod(mock.seal)

    def __init__(self) -> None:
        self.patch = _Patcher(self)
        self._patches: list[tuple[Any, list[_PatchLayer], list[Any]]] = []

    def stop(self, mock: Any) -> None:
        """Restore the patch that returned this object, leaving others active."""
        for index in range(len(self._patches) - 1, -1, -1):
            result, layers, _ = self._patches[index]
            if result is mock:
                _check_can_stop(layers)
                del self._patches[index]
                _stop_layers(layers)
                return
        raise ValueError("This object is not an active patch owned by this mocker")

    def stopall(self) -> None:
        """Restore owned patches in reverse order, without touching other owners."""
        for _, layers, _ in self._patches:
            _check_can_stop(layers)
        errors = []
        while self._patches:
            _, layers, _ = self._patches.pop()
            try:
                _stop_layers(layers)
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
        if isinstance(result, mock.NonCallableMock):
            result.reset_mock(return_value=return_value, side_effect=side_effect)
        elif inspect.isfunction(result) and isinstance(
            getattr(result, "mock", None), mock.NonCallableMock
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
        if "_karva_spy_side_effect" in vars(result):
            result.spy_return = None
            result.spy_return_iter = None
            result.spy_exception = None
            result.spy_return_list.clear()
            if side_effect:
                result.side_effect = result._karva_spy_side_effect

    def create_autospec(self, *args: Any, **kwargs: Any) -> Any:
        """Create a signature-checking mock tracked by resetall and stopall."""
        result = mock.create_autospec(*args, **kwargs)
        self._patches.append((result, [], [result]))
        return result

    def spy(self, obj: Any, name: str, *, duplicate_iterators: bool = False) -> Any:
        """Record calls, results and exceptions while running the real callable."""
        original = getattr(obj, name)

        def invoke(*args: Any, **kwargs: Any) -> Any:
            try:
                return record_return(original(*args, **kwargs))
            except BaseException as error:
                record_exception(error)
                raise

        async def async_invoke(*args: Any, **kwargs: Any) -> Any:
            try:
                return record_return(await original(*args, **kwargs))
            except BaseException as error:
                record_exception(error)
                raise

        def record_exception(error: BaseException) -> None:
            spy.spy_return = None
            spy.spy_return_iter = None
            spy.spy_exception = error

        def record_return(result: Any) -> Any:
            duplicate = None
            if duplicate_iterators and isinstance(result, Iterator):
                result, duplicate = tee(result)
            spy.spy_return = result
            spy.spy_return_iter = duplicate
            spy.spy_exception = None
            spy.spy_return_list.append(result)
            return result

        is_async = inspect.iscoroutinefunction(original)
        if not is_async and inspect.iscoroutinefunction(
            type(original).__call__ if callable(original) else None
        ):
            spy = self.patch.object(
                obj, name, new_callable=mock.AsyncMock, side_effect=async_invoke
            )
        else:
            spy = self.patch.object(
                obj,
                name,
                autospec=inspect.isfunction(original) or inspect.ismethod(original),
                side_effect=async_invoke if is_async else invoke,
            )
        spy._karva_spy_side_effect = spy.side_effect
        spy.spy_return = None
        spy.spy_return_iter = None
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
