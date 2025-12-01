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

Then running `uv run karva test` will provide the following output:

```text
test test::test_function(a=1) ... ok
test test::test_function(a=2) ... ok
test test::test_function(a=3) ... ok

test result: ok. 3 passed; 0 failed; 0 skipped; finished in 0s
```

## Multiple Variables

We can also parametrize multiple arguments:

```python title="test.py"
import karva

@karva.tags.parametrize(("a", "b"), [(1, 4), (2, 5), (3, 6)])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function(a=1, b=4) ... ok
test test::test_function(a=2, b=5) ... ok
test test::test_function(a=3, b=6) ... ok

test result: ok. 3 passed; 0 failed; 0 skipped; finished in 0s
```

Like pytest, we can put the arguments in a single string, separated by ",".

```python title="test.py"
import karva

@karva.tags.parametrize("a,b", [(1, 4), (2, 5), (3, 6)])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function(a=1, b=4) ... ok
test test::test_function(a=2, b=5) ... ok
test test::test_function(a=3, b=6) ... ok

test result: ok. 3 passed; 0 failed; 0 skipped; finished in 0s
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

Then running `uv run karva test -v` will provide the following output:

```text
test test::test_function(a=1, b=1) ... ok
test test::test_function(a=2, b=1) ... ok

test result: ok. 2 passed; 0 failed; 0 skipped; finished in 0s
```

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

Then running `uv run karva test -v` will provide the following output:

```text
test test::test_function(a=1, b=1) ... ok
test test::test_function(a=2, b=1) ... ok
test test::test_function(a=1, b=2) ... ok
test test::test_function(a=2, b=2) ... ok

test result: ok. 4 passed; 0 failed; 0 skipped; finished in 0s
```

## Params

You can use `karva.param` (similar to `pytest.param`) for parameters.

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

Then running `uv run karva test -v` will provide the following output:

```text
test tests.test_add::test_square(expected=4, input=2) ... ok
test tests.test_add::test_square ... skipped
test tests.test_add::test_square(expected=26, input=5) ... ok
test tests.test_add::test_square ... skipped
test tests.test_add::test_square(expected=50, input=7) ... ok

test result: ok. 3 passed; 0 failed; 2 skipped; finished in 0ms
```

## Pytest

You can also still use `@pytest.mark.parametrize`.

```python title="test.py"
import pytest

@pytest.mark.parametrize("a", [1, 2])
def test_function(a: int):
    assert a > 0
```

Then running `uv run karva test -v` will provide the following output:

```text
test test::test_function(a=1) ... ok
test test::test_function(a=2) ... ok

test result: ok. 2 passed; 0 failed; 0 skipped; finished in 0s
```
