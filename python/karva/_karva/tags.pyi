from collections.abc import Callable, Sequence
from typing import ParamSpec, TypeVar, overload

from karva._karva import Tags, TestFunction

_T = TypeVar("_T")
_P = ParamSpec("_P")


class _CustomTagBuilder:
    @overload
    def __call__(self, function: Callable[_P, _T], /) -> TestFunction[_P, _T]: ...
    @overload
    def __call__(self, *args: object, **kwargs: object) -> Tags: ...


def __getattr__(name: str, /) -> _CustomTagBuilder: ...


def parametrize(
    arg_names: Sequence[str] | str,
    arg_values: Sequence[Sequence[object]] | Sequence[object],
    ids: Sequence[str | None] | Callable[[object], object | None] | None = ...,
) -> Tags:
    """Parametrize the current test with the given arguments."""


def use_fixtures(*fixture_names: str) -> Tags:
    """Use the given fixtures for the current test.

    This is useful when you dont need the actual fixture
    but you need them to be called.
    """


@overload
def skip(f: Callable[_P, _T]) -> TestFunction[_P, _T]: ...
@overload
def skip(*conditions: bool, reason: str | None = ...) -> Tags:  # noqa: D418
    """Skip the current test given the conditions."""


@overload
def expect_fail(f: Callable[_P, _T]) -> TestFunction[_P, _T]: ...
@overload
def expect_fail(  # noqa: D418
    *conditions: bool,
    reason: str | None = ...,
    raises: type[BaseException] | tuple[type[BaseException], ...] | None = ...,
) -> Tags:
    """Expect the current test to fail given the conditions."""


def timeout(seconds: float) -> Tags:
    """Fail the current test if it runs longer than ``seconds``.

    Sync tests execute on the worker's main Python thread. A native watchdog
    terminates the worker when the limit expires; retries and remaining tests
    run in a fresh interpreter. Hard termination cannot guarantee fixture teardown.

    Async tests are wrapped in ``asyncio.wait_for``, which cancels the
    coroutine via ``CancelledError`` when the limit elapses.

    Fixture setup runs before the timeout starts, so slow fixtures do not
    count toward the limit.
    """


def fail_slow(seconds: float) -> Tags:
    """Fail the current test if its full lifecycle takes longer than ``seconds``.

    Unlike ``timeout``, this never kills the test early: fixture setup, the
    test call, and fixture teardown are always allowed to finish so cleanup
    is never skipped. Once the lifecycle completes, the test is reported as
    a failure if the total duration exceeded the configured budget.

    This is a coarse regression budget, not a benchmarking tool.
    """
