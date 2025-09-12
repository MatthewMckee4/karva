from collections.abc import Callable, Sequence
from typing import (
    Any,
    Generic,
    Literal,
    ParamSpec,
    TypeAlias,
    TypeVar,
    overload,
)

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

@overload
def fixture(func: Callable[_P, _T]) -> FixtureFunctionDefinition[_P, _T]: ...
@overload
def fixture(
    func: None = ...,
    *,
    scope: _ScopeName = "function",
    name: str | None = ...,
    auto_use: bool = ...,
) -> Callable[[Callable[_P, _T]], FixtureFunctionDefinition[_P, _T]]: ...

class TestFunction(Generic[_P, _T]):
    def __call__(self, *args: _P.args, **kwargs: _P.kwargs) -> _T: ...

class tag:  # noqa: N801
    class parametrize:  # noqa: N801
        arg_names: list[str]
        arg_values: list[list[Any]]

class tags:  # noqa: N801
    @classmethod
    def parametrize(
        cls,
        arg_names: Sequence[str] | str,
        arg_values: Sequence[Sequence[Any]] | Sequence[Any],
    ) -> tags: ...
    @classmethod
    def use_fixtures(cls, *fixture_names: str) -> tags: ...
    @classmethod
    def skip(cls, reason: str | None = ...) -> tags: ...
    @overload
    def __call__(self, f: tag, /) -> tags: ...
    @overload
    def __call__(self, f: Callable[_P, _T], /) -> TestFunction[_P, _T]: ...
