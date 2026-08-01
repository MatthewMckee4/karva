from collections.abc import Callable, Sequence
from typing import ParamSpec, TypeVar, overload

from karva._karva import Tags, TestFunction

_T = TypeVar("_T")
_P = ParamSpec("_P")


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
def skip(*conditions: bool, reason: str | None = ...) -> Tags:
    """Skip the current test given the conditions."""


@overload
def expect_fail(f: Callable[_P, _T]) -> TestFunction[_P, _T]: ...
@overload
def expect_fail(*conditions: bool, reason: str | None = ...) -> Tags:
    """Expect the current test to fail given the conditions."""


def timeout(seconds: float) -> Tags:
    """Fail the current test if it runs longer than ``seconds``.

    Sync tests are submitted to a single-worker ``concurrent.futures.ThreadPoolExecutor``;
    if the test does not finish within the limit, a ``TimeoutError`` is raised
    against the test and the worker thread is abandoned (Python has no safe
    way to interrupt arbitrary code, so any side effects already started will
    continue).

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
