"""Record warnings during test function execution.

Adapted from pytest's ``_pytest/recwarn.py`` (commit 8ecf49ec2). The
``recwarn`` fixture wrapper lives in ``karva._builtins`` where the
framework-fixture discoverer can see it.

The following adaptations were made:

- The ``_ispytest`` constructor parameter and ``check_ispytest`` call are
  dropped.

See the pytest license block in this repository's LICENSE file for the
applicable copyright notice.
"""

from __future__ import annotations

import builtins
import re
import warnings
from collections.abc import Callable, Iterator
from pprint import pformat
from types import TracebackType
from typing import TYPE_CHECKING, Any, ParamSpec, TypeVar, final, overload

if TYPE_CHECKING:
    from typing import Self


_P = ParamSpec("_P")
_T = TypeVar("_T")
_WarningType = type[Warning] | tuple[type[Warning], ...]
_MatchExpression = str | re.Pattern[str] | None


class WarningsRecorder(warnings.catch_warnings):
    """A context manager to record raised warnings.

    Each recorded warning is an instance of :class:`warnings.WarningMessage`.

    Adapted from :class:`warnings.catch_warnings`.
    """

    def __init__(self) -> None:
        super().__init__(record=True)
        self._entered = False
        self._list: list[warnings.WarningMessage] = []

    @property
    def list(self) -> builtins.list[warnings.WarningMessage]:
        """The list of recorded warnings."""
        return self._list

    def __getitem__(self, i: int) -> warnings.WarningMessage:
        """Get a recorded warning by index."""
        return self._list[i]

    def __iter__(self) -> Iterator[warnings.WarningMessage]:
        """Iterate through the recorded warnings."""
        return iter(self._list)

    def __len__(self) -> int:
        """Return the number of recorded warnings."""
        return len(self._list)

    def pop(self, cls: type[Warning] = Warning) -> warnings.WarningMessage:
        """Pop the first recorded warning matching ``cls``.

        Prefer an exact match over a child class of any other match.
        Raises ``AssertionError`` if there is no match.
        """
        best_idx: int | None = None
        for i, w in enumerate(self._list):
            if w.category == cls:
                return self._list.pop(i)
            if issubclass(w.category, cls) and (
                best_idx is None
                or not issubclass(w.category, self._list[best_idx].category)
            ):
                best_idx = i
        if best_idx is not None:
            return self._list.pop(best_idx)
        __tracebackhide__ = True
        raise AssertionError(f"{cls!r} not found in warning list")

    def clear(self) -> None:
        """Clear the list of recorded warnings."""
        self._list[:] = []

    def __enter__(self) -> Self:  # ty: ignore[invalid-method-override]
        if self._entered:
            __tracebackhide__ = True
            raise RuntimeError(f"Cannot enter {self!r} twice")
        _list = super().__enter__()
        assert _list is not None
        self._list = _list
        warnings.simplefilter("always")
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> None:
        if not self._entered:
            __tracebackhide__ = True
            raise RuntimeError(f"Cannot exit {self!r} without entering first")

        super().__exit__(exc_type, exc_val, exc_tb)

        # Built-in catch_warnings does not reset entered state so we do it
        # manually here for this context manager to become reusable.
        self._entered = False


@final
class WarningsChecker(WarningsRecorder):
    """Record warnings and require one matching warning."""

    def __init__(
        self,
        expected_warning: _WarningType = Warning,
        match_expr: _MatchExpression = None,
    ) -> None:
        super().__init__()

        expected_warnings = (
            expected_warning
            if isinstance(expected_warning, tuple)
            else (expected_warning,)
        )
        if not all(
            isinstance(warning_type, type) and issubclass(warning_type, Warning)
            for warning_type in expected_warnings
        ):
            raise TypeError(
                f"exceptions must be derived from Warning, not {type(expected_warning)}"
            )

        self.expected_warning = expected_warnings
        self.match_expr = match_expr

    def matches(self, warning: warnings.WarningMessage) -> bool:
        """Return whether a recorded warning matches the expectation."""
        return issubclass(warning.category, self.expected_warning) and (
            self.match_expr is None
            or re.search(self.match_expr, str(warning.message)) is not None
        )

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> None:
        super().__exit__(exc_type, exc_val, exc_tb)

        if exc_type is not None:
            return

        __tracebackhide__ = True
        emitted = pformat([record.message for record in self], indent=2)
        try:
            if not any(
                issubclass(warning.category, self.expected_warning) for warning in self
            ):
                raise AssertionError(
                    "DID NOT WARN. No warnings of type "
                    f"{self.expected_warning} were emitted.\n Emitted warnings: {emitted}."
                )
            if not any(self.matches(warning) for warning in self):
                raise AssertionError(
                    f"Regex pattern {self.match_expr!r} did not match any emitted warning.\n"
                    f" Emitted warnings: {emitted}."
                )
        finally:
            for warning in self:
                if not self.matches(warning):
                    warnings.warn_explicit(
                        message=warning.message,
                        category=warning.category,
                        filename=warning.filename,
                        lineno=warning.lineno,
                        source=warning.source,
                    )


@overload
def warns(
    expected_warning: _WarningType = Warning,
    func: None = None,
    *,
    match: _MatchExpression = None,
) -> WarningsChecker: ...


@overload
def warns(
    expected_warning: _WarningType,
    func: Callable[_P, _T],
    *args: _P.args,
    **kwargs: _P.kwargs,
) -> _T: ...


def warns(
    expected_warning: _WarningType = Warning,
    func: Callable[..., Any] | None = None,
    *args: Any,
    **kwargs: Any,
) -> WarningsChecker | Any:
    """Assert that code emits a matching warning.

    With no callable, return a context manager that records warnings. With a
    callable, invoke it with the remaining arguments and return its result.
    In callable mode, all keywords are forwarded to the callable. In context
    manager mode, ``match`` is used to match warning messages.
    """
    if func is None:
        match = kwargs.pop("match", None)
        if kwargs:
            argnames = ", ".join(sorted(kwargs))
            raise TypeError(
                f"Unexpected keyword arguments passed to karva.warns: {argnames}\n"
                "Use context-manager form instead?"
            )
        return WarningsChecker(expected_warning, match_expr=match)

    if not callable(func):
        raise TypeError(f"{func!r} object (type: {type(func)}) must be callable")
    with WarningsChecker(expected_warning):
        return func(*args, **kwargs)


_DEPRECATION_WARNINGS = (
    DeprecationWarning,
    PendingDeprecationWarning,
    FutureWarning,
)


@overload
def deprecated_call(*, match: _MatchExpression = None) -> WarningsChecker: ...


@overload
def deprecated_call(
    func: Callable[_P, _T], *args: _P.args, **kwargs: _P.kwargs
) -> _T: ...


def deprecated_call(
    func: Callable[..., Any] | None = None,
    *args: Any,
    **kwargs: Any,
) -> WarningsChecker | Any:
    """Assert that code emits a deprecation-related warning.

    With no callable, return a context manager. With a callable, invoke it
    with all supplied arguments, including a keyword named ``match``.
    """
    if func is None:
        return warns(_DEPRECATION_WARNINGS, *args, **kwargs)

    if not callable(func):
        raise TypeError(f"{func!r} object (type: {type(func)}) must be callable")
    with WarningsChecker(_DEPRECATION_WARNINGS):
        return func(*args, **kwargs)


__all__ = ["WarningsRecorder", "deprecated_call", "warns"]
