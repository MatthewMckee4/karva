# Parametrize

`@karva.tags.parametrize` runs a test with each set of inputs, like pytest.

## Basic Usage

```python title="test.py"
import karva

@karva.tags.parametrize("a", [1, 2, 3])
def test_function(a: int):
    assert a > 0
```

Running `uv run karva test` will run `test_function` three times, once for each value of `a`.

## Multiple Variables

Pass multiple argument names and a row of values for each case:

```python title="test.py"
import karva

@karva.tags.parametrize(("a", "b"), [(1, 4), (2, 5), (3, 6)])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
```

Argument names can also be comma-separated: `"a,b"`.

```python title="test.py"
import karva

@karva.tags.parametrize("a,b", [(1, 4), (2, 5), (3, 6)])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
```

## Parametrize with Fixtures

Tests can request fixtures alongside parametrized arguments:

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

Stack decorators to run every combination of their values.

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

Use `id` on `karva.param` when each row should carry its own name:

```python title="test.py"
import karva

@karva.tags.parametrize("card,expected", [
    karva.param(valid_card, "paid", id="valid-card"),
    karva.param(expired_card, "declined", id="expired-card"),
])
def test_checkout(card, expected):
    assert checkout(card) == expected
```

The ID appears in output and failure reports and provides a stable exact filter:

```console
uv run karva -E 'test(="test::test_checkout(expired-card)")'
```

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

`@pytest.mark.parametrize` is also supported:

```python title="test.py"
import pytest

@pytest.mark.parametrize("a", [1, 2])
def test_function(a: int):
    assert a > 0
```

## Original Document Positions

Adapters that generate Python tests from documentation can attach an original
position to each `karva.param`:

```python title="test_generated.py"
import karva

source = karva.SourceDocument("docs/guide.rst", ">>> 1 + 1\n2\n")

@karva.tags.parametrize("actual,expected", [
    karva.param(2, 2, id="guide-example", source=source.location(1, 1)),
])
def test_example(actual, expected):
    assert actual == expected
```

Create one `SourceDocument` per decoded document. Locations share its text
without copying the whole document per parameter case. The path identifies the
original document, which need not exist on disk. Lines and Unicode character
columns start at 1. A column immediately after the last character is allowed,
and invalid coordinates raise `ValueError`.

Failures identify the original position while retaining the Python traceback.
JUnit cases include `file`, `line`, and `column` attributes, including skipped
cases, retries, and fixture errors. Source metadata does not change collection,
filtering, cache, or coverage identity: those still refer to the generated Python
test. It does not collect documents or replay earlier examples when selecting a
later example.

Only one dimension of stacked parametrization may provide source positions.
Multiple dimensions with source positions are rejected because the original
position would be ambiguous.
