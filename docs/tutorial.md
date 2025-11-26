This tutorial will walk you through the basics of using Karva.

## Getting started

We will first create a new project using `uv`.

```bash
uv init --lib .
mkdir tests
```

This will give us a project that looks like this:

```text
.
├── pyproject.toml
├── README.md
├── src
│   └── karva_test
│       ├── __init__.py
│       └── py.typed
└── tests

```

```python title="src/calculator/__init__.py"
class Calculator:
    def add(self, a: int, b: int) -> int:
        return a + b
```

```python title="tests/test_add.py"
from calculator import Calculator

def test_add():
    calculator = Calculator()
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
