import karva
import pytest

from src import Calculator


@karva.tags.parametrize(("a", "b", "expected"), [(1, 2, 3), (2, 3, 5), (3, 4, 7)])
def test_parametrize(a: int, b: int, expected: int) -> None:
    assert a + b == expected


@karva.tags.parametrize(("a", "b", "expected"), [(1, 2, 3), (2, 3, 5), (3, 4, 7)])
def test_parametrize_with_fixture(
    calculator: Calculator,
    a: int,
    b: int,
    expected: int,
) -> None:
    assert calculator.add(a, b) == expected


@pytest.mark.parametrize(("a", "b", "expected"), [(1, 2, 3), (2, 3, 5), (3, 4, 7)])
def test_parametrize_pytest(a: int, b: int, expected: int) -> None:
    assert a + b == expected


@pytest.mark.parametrize(("a", "b", "expected"), [(1, 2, 3), (2, 3, 5), (3, 4, 7)])
def test_parametrize_with_fixture_pytest(
    calculator: Calculator,
    a: int,
    b: int,
    expected: int,
) -> None:
    assert calculator.add(a, b) == expected
