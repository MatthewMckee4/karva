from collections.abc import Callable
from typing import Any, Literal, TypeAlias, TypeVar, overload

_FixtureFunction = TypeVar("_FixtureFunction", bound=Callable[..., Any])
_ScopeName: TypeAlias = Literal["session", "module", "function"]

def karva_run() -> int: ...
@overload
def fixture(
    func: _FixtureFunction,
) -> _FixtureFunction: ...
@overload
def fixture(
    func: None = ...,
    *,
    scope: _ScopeName = "function",
    name: str | None = ...,
) -> Callable[[_FixtureFunction], _FixtureFunction]: ...
