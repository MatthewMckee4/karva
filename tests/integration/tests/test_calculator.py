from src import Calculator


def test_add(calculator: Calculator) -> None:
    assert calculator.add(1, 2) == 3


def test_subtract(calculator: Calculator) -> None:
    assert calculator.subtract(1, 2) == -1


def test_multiply(calculator: Calculator) -> None:
    assert calculator.multiply(1, 2) == 2


def test_divide(calculator: Calculator) -> None:
    assert calculator.divide(1, 2) == 0.5
