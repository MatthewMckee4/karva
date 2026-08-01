"""Record warnings during test function execution.

Adapted from pytest's ``_pytest/recwarn.py`` (commit 8ecf49ec2). Only the
``WarningsRecorder`` class is included; the ``WarningsChecker``/``warns``/
``deprecated_call`` helpers are intentionally omitted because karva does
not yet expose ``pytest.warns``. The ``recwarn`` fixture wrapper lives in
``karva._builtins`` where the framework-fixture discoverer can see it.

The following adaptations were made:

- The ``_ispytest`` constructor parameter and ``check_ispytest`` call are
  dropped.

See the pytest license block in this repository's LICENSE file for the
applicable copyright notice.
"""

from __future__ import annotations

import builtins
import warnings
from collections.abc import Iterator
from types import TracebackType
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from typing import Self


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


__all__ = ["WarningsRecorder"]
