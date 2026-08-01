"""Monkeypatching and mocking functionality.

Adapted from pytest's ``_pytest/monkeypatch.py`` (commit 8ecf49ec2). The
following adaptations were made:

- ``MonkeyPatch`` is renamed to ``MockEnv`` to match karva's public API.
- ``setitem``/``delitem`` take ``MutableMapping`` instead of ``Mapping``
  to satisfy ty (pytest's source uses ``Mapping`` to allow ``TypedDict``).
- The pytest-only ``pkg_resources`` legacy namespace package handling in
  ``syspath_prepend`` and the ``PytestWarning`` warning category are dropped.
- ``NOTSET``/``NotSetType`` are inlined as ``_NOTSET``/``_NotSetType`` instead
  of being imported from ``_pytest.compat``.
- A small ``__repr__`` is added so that karva's parametrize display shows the
  stable ``<MockEnv object>`` string instead of the default Python object repr.
- ``__enter__``/``__exit__`` are added so that ``MockEnv()`` instances can be
  used directly as context managers (pytest only exposes the classmethod
  ``MonkeyPatch.context()``).
- The module-level ``@fixture def monkeypatch(): ...`` wrapper is not present
  here — karva's framework-fixture discoverer only parses
  ``python/karva/_builtins.py``, so the fixture wrapper lives there and
  imports ``MockEnv`` from this module.

See the pytest license block in this repository's LICENSE file for the
applicable copyright notice.
"""

from __future__ import annotations

import enum
import importlib
import inspect
import os
import re
import sys
import warnings
from collections.abc import Generator, MutableMapping
from contextlib import contextmanager, suppress
from typing import Any, Final, TypeVar, final, overload


class _NotSetType(enum.Enum):
    token = 0


_NOTSET: Final = _NotSetType.token

RE_IMPORT_ERROR_NAME = re.compile(r"^No module named (.*)$")


K = TypeVar("K")
V = TypeVar("V")


def resolve(name: str) -> object:
    parts = name.split(".")

    used = parts.pop(0)
    found: object = importlib.import_module(used)
    for part in parts:
        used += "." + part
        try:
            found = getattr(found, part)
        except AttributeError:
            pass
        else:
            continue
        try:
            importlib.import_module(used)
        except ImportError as ex:
            expected = str(ex).split()[-1]
            if expected == used:
                raise
            raise ImportError(f"import error in {used}: {ex}") from ex
        found = annotated_getattr(found, part, used)
    return found


def annotated_getattr(obj: object, name: str, ann: str) -> object:
    try:
        obj = getattr(obj, name)
    except AttributeError as e:
        raise AttributeError(
            f"{type(obj).__name__!r} object at {ann} has no attribute {name!r}"
        ) from e
    return obj


def derive_importpath(import_path: str, raising: bool) -> tuple[str, object]:
    if not isinstance(import_path, str) or "." not in import_path:
        raise TypeError(f"must be absolute import path string, not {import_path!r}")
    module, attr = import_path.rsplit(".", 1)
    target = resolve(module)
    if raising:
        annotated_getattr(target, attr, ann=module)
    return attr, target


@final
class MockEnv:
    """Conveniently patch attributes, mappings, environment, and import paths.

    Returned by the :fixture:`monkeypatch` fixture. Can also be used directly
    as ``MockEnv()`` outside of a fixture; in that case, use
    :meth:`with MockEnv.context() as mp: <context>` or remember to call
    :meth:`undo` explicitly.
    """

    def __init__(self) -> None:
        self._setattr: list[tuple[object, str, object]] = []
        self._setitem: list[tuple[MutableMapping[Any, Any], object, object]] = []
        self._cwd: str | None = None
        self._savesyspath: list[str] | None = None

    def __repr__(self) -> str:
        # Stable repr that does not leak the internal module path; preserves
        # compatibility with pre-existing karva test snapshots.
        return "<MockEnv object>"

    @classmethod
    @contextmanager
    def context(cls) -> Generator[MockEnv, None, None]:
        """Create a context that undoes its patches on exit.

        The context yields a new :class:`MockEnv` instance.
        """
        m = cls()
        try:
            yield m
        finally:
            m.undo()

    @overload
    def setattr(
        self,
        target: str,
        name: object,
        value: _NotSetType = ...,
        raising: bool = ...,
    ) -> None: ...

    @overload
    def setattr(
        self,
        target: object,
        name: str,
        value: object,
        raising: bool = ...,
    ) -> None: ...

    def setattr(
        self,
        target: str | object,
        name: object | str,
        value: object = _NOTSET,
        raising: bool = True,
    ) -> None:
        """Set attribute value on target, memorizing the old value.

        For convenience, you can specify a string as ``target`` which will be
        interpreted as a dotted import path, with the last part being the
        attribute name. Raises :class:`AttributeError` if the attribute does
        not exist, unless ``raising`` is set to ``False``.
        """
        __tracebackhide__ = True
        if value is _NOTSET:
            if not isinstance(target, str):
                raise TypeError(
                    "use setattr(target, name, value) or "
                    "setattr(target, value) with target being a dotted "
                    "import string"
                )
            value = name
            name, target = derive_importpath(target, raising)
        else:
            if not isinstance(name, str):
                raise TypeError(
                    "use setattr(target, name, value) with name being a string or "
                    "setattr(target, value) with target being a dotted "
                    "import string"
                )

        oldval = getattr(target, name, _NOTSET)
        if raising and oldval is _NOTSET:
            raise AttributeError(f"{target!r} has no attribute {name!r}")

        if inspect.isclass(target):
            oldval = target.__dict__.get(name, _NOTSET)
        self._setattr.append((target, name, oldval))
        setattr(target, name, value)

    def delattr(
        self,
        target: object | str,
        name: str | _NotSetType = _NOTSET,
        raising: bool = True,
    ) -> None:
        """Delete attribute ``name`` from ``target``.

        If no ``name`` is specified and ``target`` is a string it will be
        interpreted as a dotted import path with the last part being the
        attribute name. Raises ``AttributeError`` if the attribute does not
        exist, unless ``raising`` is set to ``False``.
        """
        __tracebackhide__ = True
        if name is _NOTSET:
            if not isinstance(target, str):
                raise TypeError(
                    "use delattr(target, name) or "
                    "delattr(target) with target being a dotted "
                    "import string"
                )
            name, target = derive_importpath(target, raising)

        if not hasattr(target, name):
            if raising:
                raise AttributeError(name)
        else:
            oldval = getattr(target, name, _NOTSET)
            if inspect.isclass(target):
                oldval = target.__dict__.get(name, _NOTSET)
            self._setattr.append((target, name, oldval))
            delattr(target, name)

    def setitem(self, dic: MutableMapping[K, V], name: K, value: V) -> None:
        """Set dictionary entry ``name`` to value."""
        self._setitem.append((dic, name, dic.get(name, _NOTSET)))
        dic[name] = value

    def delitem(self, dic: MutableMapping[K, V], name: K, raising: bool = True) -> None:
        """Delete ``name`` from dict.

        Raises ``KeyError`` if it doesn't exist, unless ``raising`` is set to
        ``False``.
        """
        if name not in dic:
            if raising:
                raise KeyError(name)
        else:
            self._setitem.append((dic, name, dic.get(name, _NOTSET)))
            del dic[name]

    def setenv(self, name: str, value: object, prepend: str | None = None) -> None:
        """Set environment variable ``name`` to ``value``.

        If ``prepend`` is a character, read the current environment variable
        value and prepend the ``value`` adjoined with the ``prepend``
        character.
        """
        if not isinstance(value, str):
            warnings.warn(
                UserWarning(
                    f"Value of environment variable {name} type should be str, but got "
                    f"{value!r} (type: {type(value).__name__}); converted to str implicitly"
                ),
                stacklevel=2,
            )
            value = str(value)
        if prepend and name in os.environ:
            value = value + prepend + os.environ[name]
        self.setitem(os.environ, name, value)

    def delenv(self, name: str, raising: bool = True) -> None:
        """Delete ``name`` from the environment.

        Raises ``KeyError`` if it does not exist, unless ``raising`` is set to
        ``False``.
        """
        environ: MutableMapping[str, str] = os.environ
        self.delitem(environ, name, raising=raising)

    def syspath_prepend(self, path: str | os.PathLike[str]) -> None:
        """Prepend ``path`` to ``sys.path`` list of import locations."""
        if self._savesyspath is None:
            self._savesyspath = sys.path[:]
        sys.path.insert(0, str(path))

        # A call to syspath_prepend usually means that the caller wants to
        # import some dynamically created files, thus we invalidate the
        # import caches. This is especially important when any namespace
        # package is in use, since then the mtime based FileFinder cache
        # gets not invalidated when writing the new files quickly afterwards.
        importlib.invalidate_caches()

    def chdir(self, path: str | os.PathLike[str]) -> None:
        """Change the current working directory to the specified path."""
        if self._cwd is None:
            self._cwd = os.getcwd()
        os.chdir(path)

    def undo(self) -> None:
        """Undo previous changes.

        This call consumes the undo stack. Calling it a second time has no
        effect unless you do more monkeypatching after the undo call.
        """
        for obj, name, value in reversed(self._setattr):
            if value is not _NOTSET:
                setattr(obj, name, value)
            else:
                delattr(obj, name)
        self._setattr[:] = []
        for dictionary, key, value in reversed(self._setitem):
            if value is _NOTSET:
                with suppress(KeyError):
                    del dictionary[key]  # type: ignore[attr-defined]
            else:
                dictionary[key] = value  # type: ignore[index]
        self._setitem[:] = []
        if self._savesyspath is not None:
            sys.path[:] = self._savesyspath
            self._savesyspath = None

        if self._cwd is not None:
            os.chdir(self._cwd)
            self._cwd = None

    def __enter__(self) -> MockEnv:
        return self

    def __exit__(self, *args: object) -> None:
        self.undo()


__all__ = ["MockEnv"]
