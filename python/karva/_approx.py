"""Approximate numeric comparisons."""

from __future__ import annotations

import math
from collections.abc import Iterable, Mapping, Sequence
from decimal import Decimal
from numbers import Complex
from typing import Any, Generic, Protocol, TypeGuard, TypeVar, cast

_Tolerance = float | Decimal
_Number = Complex | Decimal
_Expected = TypeVar("_Expected")


class _Array(Protocol):
    flat: Iterable[object]
    shape: object

    def tolist(self) -> object: ...


def _is_number(value: object) -> TypeGuard[_Number]:
    return not isinstance(value, bool) and isinstance(value, Complex | Decimal)


def _is_array(value: object) -> TypeGuard[_Array]:
    return all(hasattr(value, attribute) for attribute in ("flat", "shape", "tolist"))


class _Approx(Generic[_Expected]):
    __array_priority__ = 100
    __array_ufunc__ = None
    __hash__ = None

    def __init__(
        self,
        expected: _Expected,
        rel: _Tolerance | None,
        absolute: _Tolerance | None,
        nan_ok: bool,
    ) -> None:
        self.expected = expected
        self.rel = rel
        self.abs = absolute
        self.nan_ok = nan_ok

    def __bool__(self) -> bool:
        raise AssertionError(
            "approx() is not supported in a boolean context.\n"
            "Did you mean: `assert actual == approx(expected)`?"
        )

    def __ne__(self, actual: object) -> bool:
        return not self == actual

    def _scalar(self, expected: object) -> _ApproxScalar:
        if not _is_number(expected):
            raise TypeError(
                "karva.approx() expected a numeric value, "
                f"got {type(expected).__name__}: {expected!r}"
            )
        return _ApproxScalar(expected, self.rel, self.abs, self.nan_ok)


class _ApproxScalar(_Approx[_Number]):
    def __eq__(self, actual: object) -> bool:
        if _is_array(actual):
            return all(self == _item(value) for value in actual.flat)
        if actual == self.expected:
            return not isinstance(actual, bool) or isinstance(self.expected, bool)
        if not _is_number(actual):
            return False
        if math.isnan(abs(self.expected)):
            return self.nan_ok and math.isnan(abs(actual))
        if math.isinf(abs(self.expected)):
            return False
        try:
            expected: Any = self.expected
            return abs(expected - actual) <= self.tolerance
        except TypeError:
            return False

    @property
    def tolerance(self) -> _Tolerance:
        if isinstance(self.expected, Decimal):
            default_absolute = Decimal("1e-12")
            default_relative = Decimal("1e-6")
        else:
            default_absolute = 1e-12
            default_relative = 1e-6

        absolute = self.abs if self.abs is not None else default_absolute
        if absolute < 0:
            raise ValueError(f"absolute tolerance can't be negative: {absolute}")
        if math.isnan(absolute):
            raise ValueError("absolute tolerance can't be NaN")
        if self.abs is not None and self.rel is None:
            return absolute

        relative = self.rel if self.rel is not None else default_relative
        try:
            expected: Any = self.expected
            relative_tolerance = relative * abs(expected)
        except TypeError as error:
            raise TypeError("Decimal comparisons require Decimal tolerances") from error
        if relative_tolerance < 0:
            raise ValueError(
                f"relative tolerance can't be negative: {relative_tolerance}"
            )
        if math.isnan(relative_tolerance):
            raise ValueError("relative tolerance can't be NaN")
        return max(relative_tolerance, absolute)

    def __repr__(self) -> str:
        if math.isinf(abs(self.expected)):
            return str(self.expected)
        try:
            tolerance = self.tolerance
            formatted = (
                f"{tolerance:n}"
                if Decimal("1e-3") <= tolerance < Decimal("1e3")
                else f"{tolerance:.1e}"
            )
        except (TypeError, ValueError):
            formatted = "???"
        if isinstance(self.expected, complex) and self.expected.imag:
            formatted += " ∠ ±180°"
        return f"{self.expected} ± {formatted}"


class _ApproxSequence(_Approx[Sequence[object]]):
    def __eq__(self, actual: object) -> bool:
        if not isinstance(actual, Sequence) or isinstance(actual, str | bytes):
            return False
        return len(actual) == len(self.expected) and all(
            self._scalar(expected) == value
            for value, expected in zip(actual, self.expected, strict=True)
        )

    def __repr__(self) -> str:
        values = (self._scalar(value) for value in self.expected)
        if type(self.expected) is tuple:
            return f"approx({tuple(values)!r})"
        return f"approx({list(values)!r})"


class _ApproxMapping(_Approx[Mapping[object, object]]):
    def __eq__(self, actual: object) -> bool:
        if not isinstance(actual, Mapping) or actual.keys() != self.expected.keys():
            return False
        actual_mapping = cast(Mapping[Any, object], actual)
        return all(
            self._scalar(expected) == actual_mapping[key]
            for key, expected in self.expected.items()
        )

    def __repr__(self) -> str:
        values = {key: self._scalar(value) for key, value in self.expected.items()}
        return f"approx({values!r})"


class _ApproxArray(_Approx[_Array]):
    def __eq__(self, actual: object) -> bool:
        if _is_number(actual):
            return all(
                self._scalar(_item(value)) == actual for value in self.expected.flat
            )
        if not _is_array(actual) or actual.shape != self.expected.shape:
            return False
        return all(
            self._scalar(_item(expected)) == _item(value)
            for value, expected in zip(actual.flat, self.expected.flat, strict=True)
        )

    def __repr__(self) -> str:
        return f"approx({self.expected.tolist()!r})"


def _item(value: object) -> object:
    item = getattr(value, "item", None)
    return item() if item is not None else value


def _validate_values(values: Iterable[object]) -> None:
    for index, value in enumerate(values):
        item = _item(value)
        if not _is_number(item):
            raise TypeError(
                "karva.approx() expected a numeric value "
                f"at index {index}, got {type(item).__name__}: {item!r}"
            )


def approx(
    expected: object,
    rel: _Tolerance | None = None,
    abs: _Tolerance | None = None,
    nan_ok: bool = False,
) -> _Approx[Any]:
    """Return an object that compares equal to numbers within given tolerances."""
    if isinstance(expected, Mapping):
        for key, value in expected.items():
            item = _item(value)
            if not _is_number(item):
                raise TypeError(
                    "karva.approx() expected a numeric value "
                    f"at key {key!r}, got {type(item).__name__}: {item!r}"
                )
        return _ApproxMapping(cast(Mapping[object, object], expected), rel, abs, nan_ok)
    if _is_array(expected):
        _validate_values(expected.flat)
        return _ApproxArray(expected, rel, abs, nan_ok)
    if isinstance(expected, Sequence) and not isinstance(expected, str | bytes):
        _validate_values(expected)
        return _ApproxSequence(expected, rel, abs, nan_ok)
    if not _is_number(expected):
        raise TypeError(
            "karva.approx() expected a numeric value, "
            f"got {type(expected).__name__}: {expected!r}"
        )
    return _ApproxScalar(expected, rel, abs, nan_ok)


__all__ = ["approx"]
