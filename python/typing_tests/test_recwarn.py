import warnings
from typing import assert_type

import karva
from karva._fixtures.recwarn import WarningsChecker


def emit_warning(value: int, *, label: str, match: str) -> int:
    """Emit warning and return value."""
    warnings.warn(f"{label}: {match}", UserWarning, stacklevel=2)
    return value


def emit_deprecation(value: int, *, match: str) -> int:
    """Emit deprecation warning and return value."""
    warnings.warn(match, DeprecationWarning, stacklevel=2)
    return value


assert_type(karva.warns(UserWarning), WarningsChecker)
assert_type(
    karva.warns(UserWarning, emit_warning, 42, label="value", match="keyword"),
    int,
)
assert_type(karva.deprecated_call(emit_deprecation, 42, match="deprecated"), int)
