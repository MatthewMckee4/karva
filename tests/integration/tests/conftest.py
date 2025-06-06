import karva

from src import Calculator


@karva.fixture
def calculator() -> Calculator:
    return Calculator()
