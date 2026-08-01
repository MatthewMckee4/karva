import builtins
import types
from collections.abc import Callable, Sequence
from typing import Generic, Literal, NoReturn, Self, TypeAlias, TypeVar, overload

from typing_extensions import ParamSpec

_ScopeName: TypeAlias = Literal["session", "package", "module", "function"]

_T = TypeVar("_T")
_P = ParamSpec("_P")


def karva_run() -> int:
    """Run Karva using the current process arguments."""


class FixtureFunctionMarker(Generic[_P, _T]):
    """Mark a function as a fixture."""

    def __call__(
        self,
        function: Callable[_P, _T],
    ) -> FixtureFunctionDefinition[_P, _T]:
        """Mark the function as a fixture."""


class FixtureFunctionDefinition(Generic[_P, _T]):
    """Represent a registered fixture function."""

    def __call__(self, *args: _P.args, **kwargs: _P.kwargs) -> _T:
        """Call the underlying fixture function."""


@overload
def fixture(func: Callable[_P, _T]) -> FixtureFunctionDefinition[_P, _T]: ...
@overload
def fixture(
    func: None = ...,
    *,
    scope: _ScopeName = "function",
    name: str | None = ...,
    auto_use: bool = ...,
) -> FixtureFunctionMarker[_P, _T]: ...


class TestFunction(Generic[_P, _T]):
    """Represent a registered test function."""

    def __call__(self, *args: _P.args, **kwargs: _P.kwargs) -> _T:
        """Call the underlying test function."""


class Tags:
    """Decorate a test function with configured tags."""

    def __call__(self, f: Callable[_P, _T], /) -> Callable[_P, _T]:
        """Apply the tags to a test function."""


def skip(reason: str | None = ...) -> NoReturn:
    """Skip the current test."""


def fail(reason: str | None = ...) -> NoReturn:
    """Fail the current test."""


class Param:
    """Represent values and tags for a parametrized test case."""

    @property
    def values(self) -> list[object]:
        """The values to parameterize the test case with."""


def param(
    *values: object, tags: Sequence[Tags | Callable[[], Tags]] | None = None
) -> None:
    """Define a parameterized test case.

    Args:
        *values: The values to parameterize the test case with.
        tags: The tag or tag functions.

    Examples:
        Parameterize a test and tag individual cases:

        ```python
        import karva

        @karva.tags.parametrize("input,expected", [
            karva.param(2, 4),
            karva.param(4, 17, tags=(karva.tags.skip,)),
            karva.param(5, 26, tags=(karva.tags.expect_fail,)),
            karva.param(6, 36, tags=(karva.tags.skip(True),)),
            karva.param(7, 50, tags=(karva.tags.expect_fail(True),)),
        ])
        def test_square(input, expected):
            assert input ** 2 == expected
        ```
    """


class ExceptionInfo:
    """Stores information about a caught exception from `karva.raises`."""

    @property
    def type(self) -> builtins.type[BaseException] | None:
        """The exception type."""

    @property
    def value(self) -> BaseException | None:
        """The exception instance."""

    @property
    def tb(self) -> object | None:
        """The traceback object."""


class RaisesContext:
    """Context manager returned by `karva.raises`."""

    def __enter__(self) -> ExceptionInfo:
        """Enter the exception assertion context."""

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: types.TracebackType | None,
    ) -> bool:
        """Exit the exception assertion context."""


def raises(
    expected_exception: type[BaseException],
    *,
    match: str | None = None,
) -> RaisesContext:
    """Assert that a block of code raises a specific exception.

    Args:
        expected_exception: The expected exception type.
        match: An optional regex pattern to match against the string
            representation of the exception.

    """


@overload
def assert_snapshot(
    value: object,
    *,
    inline: str | None = None,
) -> None: ...
@overload
def assert_snapshot(
    value: object,
    *,
    name: str,
) -> None: ...
@overload
def assert_json_snapshot(
    value: object,
    *,
    inline: str | None = None,
) -> None: ...
@overload
def assert_json_snapshot(
    value: object,
    *,
    name: str,
) -> None: ...


class Command:
    """Builder for running external commands in snapshot tests.

    Wraps a command with its arguments, environment, stdin, and working
    directory. Passed to `assert_cmd_snapshot` to capture and snapshot
    the command's stdout, stderr, and exit code.
    """

    def __init__(self, program: str) -> None:
        """Create a command for the given program."""

    def arg(self, value: str) -> Self:
        """Append an argument."""

    def args(self, values: Sequence[str]) -> Self:
        """Append multiple arguments."""

    def env(self, key: str, value: str) -> Self:
        """Set an environment variable."""

    def envs(self, vars: dict[str, str]) -> Self:
        """Set multiple environment variables."""

    def current_dir(self, path: str) -> Self:
        """Set the working directory."""

    def stdin(self, data: str) -> Self:
        """Set the standard input data."""


@overload
def assert_cmd_snapshot(
    cmd: Command,
    *,
    inline: str | None = None,
) -> None: ...
@overload
def assert_cmd_snapshot(
    cmd: Command,
    *,
    name: str,
) -> None: ...


class SnapshotSettings:
    """Context manager for scoped snapshot configuration.

    Filters are applied sequentially to the serialized snapshot value before
    comparison/storage. Nesting accumulates filters from outer to inner scope.
    """

    def __init__(
        self,
        *,
        filters: list[tuple[str, str]] | None = None,
        allow_duplicates: bool = False,
    ) -> None:
        """Create scoped snapshot settings."""

    def __enter__(self) -> Self:
        """Enter the snapshot settings context."""

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: types.TracebackType | None,
    ) -> bool:
        """Exit the snapshot settings context."""


def snapshot_settings(
    *,
    filters: list[tuple[str, str]] | None = None,
    allow_duplicates: bool = False,
) -> SnapshotSettings:
    """Create a context manager for scoped snapshot configuration.

    Args:
        filters: List of (regex_pattern, replacement) pairs applied sequentially
            to the serialized snapshot value before comparison/storage.
        allow_duplicates: If True, allow multiple unnamed snapshots in a single test.

    """


class SkipError(Exception):
    """Raised when `karva.skip` is called."""


class FailError(Exception):
    """Raised when `karva.fail` is called."""


class SnapshotMismatchError(Exception):
    """Raised when a snapshot assertion fails."""


class InvalidFixtureError(Exception):
    """Raised when an invalid fixture is encountered."""
