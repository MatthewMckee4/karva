The `parametrize` tag allows us to run the same test with several different inputs.

This works like pytest's `parametrize` decorator.

## Basic Usage

First, here is a small example:

```python title="test.py"
import karva

@karva.tags.parametrize("a", [1, 2, 3])
def test_function(a: int):
    assert a > 0
```

Running `uv run karva test` will run `test_function` three times, once for each value of `a`.

## Multiple Variables

We can also parametrize multiple arguments:

```python title="test.py"
import karva

@karva.tags.parametrize(("a", "b"), [(1, 4), (2, 5), (3, 6)])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
```

Like pytest, we can put the arguments in a single string, separated by ",":

```python title="test.py"
import karva

@karva.tags.parametrize("a,b", [(1, 4), (2, 5), (3, 6)])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
```

## Parametrize with Fixtures

We can also mix fixtures and parametrize:

```python title="test.py"
import karva

@karva.fixture
def b() -> int:
    return 1

@karva.tags.parametrize("a", [1, 2])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
```

Each parametrized variant receives the fixture value alongside the parametrized arguments.

## Multiple Parametrize Tags

We can also use multiple decorators, allowing us to test more scenarios.
This will result in a cartesian product of the parametrize values.

```python title="test.py"
import karva

@karva.tags.parametrize("a", [1, 2])
@karva.tags.parametrize("b", [1, 2])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
```

This runs `test_function` four times with all combinations of `a` and `b`.

## IDs

Use `ids` to give parameter sets stable, readable names:

```python title="test.py"
import karva

@karva.tags.parametrize(
    "card,expected",
    [(valid_card, "paid"), (expired_card, "declined")],
    ids=["valid-card", "expired-card"],
)
def test_checkout(card, expected):
    assert checkout(card) == expected
```

`ids` can also be a function. It is called for each parameter value, and returned
parts are joined with `-`:

```python title="test.py"
import karva

@karva.tags.parametrize(
    "status,expected",
    [("paid", True), ("declined", False)],
    ids=lambda value: value.upper() if isinstance(value, str) else None,
)
def test_status(status, expected):
    assert is_successful(status) is expected
```

IDs combine with `-` when multiple parametrize tags are stacked.

## Params

You can use `karva.param` (similar to `pytest.param`) to attach tags to individual parameter sets:

```python title="test.py"
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
```

## Pytest

You can also still use `@pytest.mark.parametrize`:

```python title="test.py"
import pytest

@pytest.mark.parametrize("a", [1, 2])
def test_function(a: int):
    assert a > 0
```

Pytest's `indirect=True` and `indirect=[...]` forms route parameter values
through fixtures and expose them as `request.param`:

```python title="test.py"
import pytest


@pytest.fixture
def database(request):
    return connect(request.param)


@pytest.mark.parametrize("database", ["sqlite", "postgres"], indirect=True)
def test_query(database):
    assert database.query("select 1") == 1
```
