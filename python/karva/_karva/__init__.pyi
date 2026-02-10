import types
from collections.abc import Callable, Sequence
from typing import Generic, Literal, NoReturn, Self, TypeAlias, TypeVar, overload

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
    def __call__(self, *args: _P.args, **kwargs: _P.kwargs) -> _T: ...

class Tags:
    def __call__(self, f: Callable[_P, _T], /) -> Callable[_P, _T]: ...

def skip(reason: str | None = ...) -> NoReturn:
    """Skip the current test."""

def fail(reason: str | None = ...) -> NoReturn:
    """Fail the current test."""

class Param:
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

    .. code-block:: python

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
    """

class ExceptionInfo:
    """Stores information about a caught exception from `karva.raises`."""

    @property
    def type(self) -> type[BaseException] | None:
        """The exception type."""

    @property
    def value(self) -> BaseException | None:
        """The exception instance."""

    @property
    def tb(self) -> object | None:
        """The traceback object."""

class RaisesContext:
    """Context manager returned by `karva.raises`."""

    def __enter__(self) -> ExceptionInfo: ...
    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: types.TracebackType | None,
    ) -> bool: ...

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

class SnapshotSettings:
    """Context manager for scoped snapshot configuration.

    Filters are applied sequentially to the serialized snapshot value before
    comparison/storage. Nesting accumulates filters from outer to inner scope.
    """

    def __init__(
        self,
        *,
        filters: list[tuple[str, str]] | None = None,
    ) -> None: ...
    def __enter__(self) -> Self: ...
    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: types.TracebackType | None,
    ) -> bool: ...

def snapshot_settings(
    *,
    filters: list[tuple[str, str]] | None = None,
) -> SnapshotSettings:
    """Create a context manager for scoped snapshot configuration.

    Args:
        filters: List of (regex_pattern, replacement) pairs applied sequentially
            to the serialized snapshot value before comparison/storage.
    """

class SkipError(Exception):
    """Raised when `karva.skip` is called."""

class FailError(Exception):
    """Raised when `karva.fail` is called."""

class InvalidFixtureError(Exception):
    """Raised when an invalid fixture is encountered."""

class MockEnv:
    """Helper to conveniently patch attributes/items/environment variables/syspath.

    This class is compatible with pytest's monkeypatch fixture.
    """

    def __init__(self) -> None: ...
    @classmethod
    def context(cls) -> MockEnv:
        """Context manager that returns a new Mock object which undoes any patching
        done inside the with block upon exit.
        """

    @overload
    def setattr(
        self,
        target: object,
        name: str,
        value: object,
        raising: bool = True,
    ) -> None:
        """Set attribute value on target, memorising the old value.

        Args:
            target: The object to set the attribute on
            name: The attribute name
            value: The value to set
            raising: Whether to raise an error if the attribute doesn't exist
        """

    @overload
    def setattr(
        self,
        target: str,
        name: object,
        value: None = None,
        raising: bool = True,
    ) -> None:
        """Set attribute using dotted import path.

        Args:
            target: Dotted import path (e.g., 'os.path.exists')
            name: The value to set
            value: Not used in this overload
            raising: Whether to raise an error if the attribute doesn't exist
        """

    @overload
    def delattr(
        self,
        target: object,
        name: str,
        raising: bool = True,
    ) -> None:
        """Delete attribute from target.

        Args:
            target: The object to delete the attribute from
            name: The attribute name
            raising: Whether to raise an error if the attribute doesn't exist
        """

    @overload
    def delattr(
        self,
        target: str,
        name: None = None,
        raising: bool = True,
    ) -> None:
        """Delete attribute using dotted import path.

        Args:
            target: Dotted import path (e.g., 'os.path.exists')
            name: Not used in this overload
            raising: Whether to raise an error if the attribute doesn't exist
        """

    def setitem(
        self,
        dic: dict[object, object],
        name: object,
        value: object,
    ) -> None:
        """Set dictionary entry name to value.

        Args:
            dic: The dictionary to modify
            name: The key
            value: The value to set
        """

    def delitem(
        self,
        dic: dict[object, object],
        name: object,
        raising: bool = True,
    ) -> None:
        """Delete name from dict.

        Args:
            dic: The dictionary to modify
            name: The key to delete
            raising: Whether to raise an error if the key doesn't exist
        """

    def setenv(
        self,
        name: str,
        value: object,
        prepend: str | None = None,
    ) -> None:
        """Set environment variable name to value.

        Args:
            name: The environment variable name
            value: The value to set (will be converted to string)
            prepend: If provided, prepend value with this separator to existing value
        """

    def delenv(
        self,
        name: str,
        raising: bool = True,
    ) -> None:
        """Delete environment variable.

        Args:
            name: The environment variable name
            raising: Whether to raise an error if the variable doesn't exist
        """

    def syspath_prepend(self, path: str) -> None:
        """Prepend path to sys.path list of import locations.

        Args:
            path: The path to prepend
        """

    def chdir(self, path: str | object) -> None:
        """Change the current working directory to the specified path.

        Args:
            path: The path to change to (string or Path object)
        """

    def undo(self) -> None:
        """Undo all changes made by this Mock instance."""

    def __enter__(self) -> Self: ...
    def __exit__(
        self,
        exc_type: object,
        exc_val: object,
        exc_tb: object,
    ) -> bool: ...
