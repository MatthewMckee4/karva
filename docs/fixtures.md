## Constraints

We do not discover tests from a `conftest.py` unless it is explicitly specified.

We do not support fixtures that require other fixtures (yet).

## Example

```bash
uv init --lib calculator
```

This will give us a project that looks like this:

```
calculator
├── pyproject.toml
├── README.md
└── src
    └── calculator
        ├── __init__.py
        └── py.typed
```

We can then create our core logic in `src/calculator/__init__.py`.

```py
class Calculator:
    def add(self, a: int, b: int) -> int:
        return a + b
```

We can then create our fixtures in `tests/conftest.py`.

```py
from karva import fixture

@fixture
def calculator() -> Calculator:
    return Calculator()
```

We can then create our tests in `tests/test_add.py`.

```py
from calculator import Calculator

def test_add(calculator: Calculator):
    assert calculator.add(1, 2) == 3
```

Then, we'll add karva to our project.

```bash
uv add --dev karva
```

We can then run our tests with `uv run karva test`.

```bash
uv run karva test
```

This will give us a report of our tests.

```
All checks passed!
```
