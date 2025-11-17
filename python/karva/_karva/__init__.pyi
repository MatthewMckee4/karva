from collections.abc import Callable, Sequence
from typing import Generic, Literal, NoReturn, TypeAlias, TypeVar, overload

from typing_extensions import ParamSpec

_ScopeName: TypeAlias = Literal["session", "package", "module", "function"]

_T = TypeVar("_T")
_P = ParamSpec("_P")

def karva_run() -> int: ...

class FixtureFunctionMarker(Generic[_P, _T]):
    def __call__(
        self,
        function: Callable[_P, _T],
    ) -> FixtureFunctionDefinition[_P, _T]: ...

class FixtureFunctionDefinition(Generic[_P, _T]):
    def __call__(self, *args: _P.args, **kwargs: _P.kwargs) -> _T: ...

class FixtureRequest:
    @property
    def param(self) -> object: ...

@overload
def fixture(func: Callable[_P, _T]) -> FixtureFunctionDefinition[_P, _T]: ...
@overload
def fixture(
    func: None = ...,
    *,
    scope: _ScopeName = "function",
    name: str | None = ...,
    auto_use: bool = ...,
    params: Sequence[object] | None = ...,
) -> FixtureFunctionMarker[_P, _T]: ...

class Tags:
    def __call__(self, f: Callable[_P, _T], /) -> Callable[_P, _T]: ...

def skip(reason: str | None = ...) -> NoReturn:
    """Skip the current test."""

def fail(reason: str | None = ...) -> NoReturn:
    """Fail the current test."""

class SkipError(Exception):
    """Raised when `karva.skip` is called."""

class FailError(Exception):
    """Raised when `karva.fail` is called."""
