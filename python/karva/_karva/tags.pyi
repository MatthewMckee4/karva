"""Type definitions for Karva test tags."""

from __future__ import annotations

from typing import TYPE_CHECKING, ParamSpec, TypeVar, overload

if TYPE_CHECKING:
    from collections.abc import Callable, Sequence

    from karva._karva import Tags, TestFunction

_T = TypeVar("_T")
_P = ParamSpec("_P")


def parametrize(
    arg_names: Sequence[str] | str,
    arg_values: Sequence[Sequence[object]] | Sequence[object],
    ids: Sequence[str | None] | Callable[[object], object | None] | None = ...,
) -> Tags:
    """Parameterize a test function with values."""


def use_fixtures(*fixture_names: str) -> Tags:
    """Inject named fixtures into a test function."""


@overload
def skip(f: Callable[_P, _T]) -> TestFunction[_P, _T]: ...
@overload
def skip(*conditions: bool, reason: str | None = ...) -> Tags: ...


@overload
def expect_fail(f: Callable[_P, _T]) -> TestFunction[_P, _T]: ...
@overload
def expect_fail(*conditions: bool, reason: str | None = ...) -> Tags: ...


def timeout(seconds: float) -> Tags:
    """Fail a test if it exceeds the given duration."""


def fail_slow(seconds: float) -> Tags:
    """Fail a completed test if it exceeded the given duration."""
