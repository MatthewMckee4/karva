"""Python-facing objects for the test and fixture request lifecycle."""

import sys
from collections import ChainMap
from collections.abc import Callable, Iterator, Mapping
from functools import cache
from pathlib import Path
from types import ModuleType
from typing import Any, NoReturn

from karva._karva import _FixtureRequestState


class FixtureLookupError(LookupError):
    """A fixture could not be resolved in its request context."""


@cache
def _compatible_lookup_error(pytest_error: type[Exception]) -> type[FixtureLookupError]:
    return type(
        "FixtureLookupError",
        (FixtureLookupError, pytest_error),
        {
            "__module__": "karva",
            "__init__": LookupError.__init__,
            "__str__": LookupError.__str__,
        },
    )


def _lookup_error(message: str | None) -> FixtureLookupError:
    pytest = sys.modules.get("pytest")
    if pytest is None:
        return FixtureLookupError(message)
    return _compatible_lookup_error(pytest.FixtureLookupError)(message)


class RequestConfig:
    """Resolved Karva settings for this worker."""

    def __init__(self, rootpath: Path, settings: Mapping[str, Any]) -> None:
        self.rootpath = rootpath
        self.settings = settings

    def getoption(self, name: str, default: Any = None) -> Any:
        """Get a setting by section and name, or by an unambiguous option name."""
        name = name.removeprefix("--").replace("_", "-")
        section, dot, option = name.partition(".")
        if dot:
            return self.settings.get(section, {}).get(option, default)
        values = [
            section_value[name]
            for section_value in self.settings.values()
            if isinstance(section_value, dict) and name in section_value
        ]
        return values[0] if len(values) == 1 else default


class RequestNode:
    """A test invocation or a scope that owns fixtures and markers."""

    def __init__(
        self,
        name: str,
        nodeid: str,
        path: Path,
        parent: "RequestNode | None",
        config: RequestConfig,
        obj: Any = None,
    ) -> None:
        self.name = name
        self.nodeid = nodeid
        self.path = path
        self.parent = parent
        self.config = config
        self.obj = obj
        local_keywords: dict[str, Any] = {name: True}
        self.keywords = (
            ChainMap(local_keywords)
            if parent is None
            else parent.keywords.new_child(local_keywords)
        )
        self.own_markers: list[Any] = []
        self._runtime_markers: list[Any] = []
        self.session = self if parent is None else parent.session

    def add_marker(self, marker: Any, append: bool = True) -> None:
        """Add a marker to this node."""
        self._add_marker(marker, append)
        if append:
            self._runtime_markers.append(marker)
        else:
            self._runtime_markers.insert(0, marker)

    def _add_marker(self, marker: Any, append: bool = True) -> None:
        mark = getattr(marker, "mark", marker)
        name = mark if isinstance(mark, str) else getattr(mark, "name", None)
        if not isinstance(name, str):
            msg = "expected a marker name or marker object"
            raise TypeError(msg)
        self.keywords[name] = marker
        if append:
            self.own_markers.append(mark)
        else:
            self.own_markers.insert(0, mark)

    def iter_markers(self, name: str | None = None) -> Iterator[Any]:
        """Iterate this node's markers, followed by markers on its parents."""
        node: RequestNode | None = self
        while node is not None:
            for marker in node.own_markers:
                marker_name = marker if isinstance(marker, str) else marker.name
                if name is None or marker_name == name:
                    yield marker
            node = node.parent

    def get_closest_marker(self, name: str, default: Any = None) -> Any:
        """Return the nearest matching marker."""
        return next(self.iter_markers(name), default)

    def _all_runtime_markers(self) -> Iterator[Any]:
        yield from self._runtime_markers
        if self.parent is not None:
            yield from self.parent._all_runtime_markers()


class FixtureRequest:
    """Context for a running test or one instance of a fixture."""

    param: Any

    def __init__(
        self,
        state: _FixtureRequestState,
        node: RequestNode,
        test_node: RequestNode,
        module: ModuleType | None,
        path: Path,
    ) -> None:
        self._state = state
        self.node = node
        self._test_node = test_node
        self._module = module
        self._path = path

    def __repr__(self) -> str:
        """Return the request owner without exposing object addresses."""
        return f"<FixtureRequest {self.fixturename!r}>"

    @property
    def scope(self) -> str:
        return self._state.scope

    @property
    def fixturename(self) -> str | None:
        return self._state.fixturename

    @property
    def fixturenames(self) -> list[str]:
        return self._state.fixturenames

    @property
    def config(self) -> RequestConfig:
        return self.node.config

    @property
    def session(self) -> RequestNode:
        return self.node.session

    @property
    def keywords(self) -> ChainMap[str, Any]:
        return self.node.keywords

    @property
    def function(self) -> Callable[..., Any]:
        self._require_scope("function", ("function",))
        return self._test_node.obj

    @property
    def cls(self) -> None:
        self._require_scope("cls", ("function",))
        return None

    @property
    def instance(self) -> None:
        return None

    @property
    def module(self) -> ModuleType:
        self._require_scope("module", ("function", "module"))
        if self._module is None:
            msg = "module is unavailable outside a test module"
            raise AttributeError(msg)
        return self._module

    @property
    def path(self) -> Path:
        self._require_scope("path", ("function", "module", "package"))
        return self._path

    def _require_scope(self, attribute: str, scopes: tuple[str, ...]) -> None:
        if self.scope not in scopes:
            msg = f"{attribute} not available in {self.scope}-scoped context"
            raise AttributeError(msg)

    def addfinalizer(self, finalizer: Callable[[], object]) -> None:
        """Register a teardown callback for this request."""
        self._state.addfinalizer(finalizer)

    def applymarker(self, marker: Any) -> None:
        """Apply a marker to this request's node."""
        self.node.add_marker(marker)

    def getfixturevalue(self, argname: str) -> Any:
        """Resolve and return a fixture by name."""
        if argname == "request":
            return self
        return self._state.getfixturevalue(argname)

    def raiseerror(self, msg: str | None) -> NoReturn:
        """Raise a fixture lookup error with a custom message."""
        raise _lookup_error(msg)
