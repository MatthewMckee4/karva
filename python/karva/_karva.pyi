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
_Scope: TypeAlias = _ScopeName | Callable[[str], _ScopeName]

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
    scope: _Scope = "function",
    name: str | None = ...,
    auto_use: bool = ...,
) -> Callable[[Callable[_P, _T]], FixtureFunctionDefinition[_P, _T]]:
    """Decorator for a fixture function.

    Args:
        func: The fixture function.
        scope: The scope of the fixture.
            This can either be a string value or a callable that takes the fixture
            name and returns a string value.
        name: The name of the fixture.
        auto_use: Whether to automatically use the fixture.

    Returns:
        _description_
    """

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
    @overload
    def __call__(self, f: tag, /) -> tags: ...
    @overload
    def __call__(self, f: Callable[_P, _T], /) -> TestFunction[_P, _T]: ...
