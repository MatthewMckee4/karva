# ruff: noqa: N801
from collections.abc import Callable, Sequence
from typing import Any, Generic, Literal, NoReturn, TypeAlias, TypeVar, overload

from typing_extensions import ParamSpec

_ScopeName: TypeAlias = Literal["session", "package", "module", "function"]

_T = TypeVar("_T")
_P = ParamSpec("_P")

def karva_run() -> int: ...

class FixtureFunctionMarker(Generic[_P, _T]):
    def __init__(self, scope: str = "function", name: str | None = None) -> None: ...
    def __call__(
        self,
        function: Callable[_P, _T],
    ) -> FixtureFunctionDefinition[_P, _T]: ...

class FixtureFunctionDefinition(Generic[_P, _T]):
    def __init__(self, function: Callable[_P, _T], name: str, scope: str) -> None: ...
    def __call__(self, *args: _P.args, **kwargs: _P.kwargs) -> _T: ...

class FixtureRequest:
    @property
    def param(self) -> Any: ...

@overload
def fixture(func: Callable[_P, _T]) -> FixtureFunctionDefinition[_P, _T]: ...
@overload
def fixture(
    func: None = ...,
    *,
    scope: _ScopeName = "function",
    name: str | None = ...,
    auto_use: bool = ...,
    params: Sequence[Any] | None = ...,
) -> Callable[[Callable[_P, _T]], FixtureFunctionDefinition[_P, _T]]: ...

class tags:
    @classmethod
    def parametrize(
        cls,
        arg_names: Sequence[str] | str,
        arg_values: Sequence[Sequence[Any]] | Sequence[Any],
    ) -> tags: ...
    @classmethod
    def use_fixtures(cls, *fixture_names: str) -> tags:
        """Use the given fixtures for the current test.

        This is useful when you dont need the actual fixture
        but you need them to be called.
        """

    @classmethod
    def skip(cls, *conditions: bool, reason: str | None = ...) -> tags:
        """Skip the current test given the conditions."""
    @classmethod
    def expect_fail(cls, *conditions: bool, reason: str | None = ...) -> tags:
        """Expect the current test to fail given the conditions."""

    def __call__(self, f: Callable[_P, _T], /) -> Callable[_P, _T]: ...

def skip(reason: str | None = ...) -> NoReturn:
    """Skip the current test."""

def fail(reason: str | None = ...) -> NoReturn:
    """Fail the current test."""

class SkipError(Exception):
    """Raised when `karva.skip` is called."""

class FailError(Exception):
    """Raised when `karva.fail` is called."""
