## Skip

If you want to skip a test when its running, use `karva.skip()`.

```python title="test.py"
import karva

def test_function():
    karva.skip()
```

You can optionally provide a reason for skipping the test by passing it as an argument to `karva.skip()`.

```python title="test.py"
import karva

def test_function():
    karva.skip("This test is not ready yet")

def test_function2():
    karva.skip(reason="This test is not ready yet")
```

You can still use `pytest.skip()` to skip tests.

## Fail

If you want to fail a test when its running, use `karva.fail()`.

```python title="test.py"
import karva

def test_function():
    karva.fail()
```

You can optionally provide a reason for failing the test by passing it as an argument to `karva.fail()`.

```python title="test.py"
import karva

def test_function():
    karva.fail("This test is not ready yet")

def test_function2():
    karva.fail(reason="This test is not ready yet")
```

Then running `uv run karva test` will result in two test fails.

You can still use `pytest.fail()` to fail tests.

## Raises

If you want to assert that a block of code raises a specific exception, use `karva.raises()`.

```python title="test.py"
import karva

def test_function():
    with karva.raises(ValueError):
        raise ValueError("something went wrong")
```

You can optionally provide a `match` parameter to match a regex pattern against the string representation of the exception.

```python title="test.py"
import karva

def test_function():
    with karva.raises(ValueError, match="something"):
        raise ValueError("something went wrong")
```

You can access the exception info by using the `as` keyword. The returned object has `type`, `value`, and `tb` properties.

```python title="test.py"
import karva

def test_function():
    with karva.raises(ValueError) as exc_info:
        raise ValueError("something went wrong")

    assert exc_info.type is ValueError
    assert str(exc_info.value) == "something went wrong"
    assert exc_info.tb is not None
```

You can still use `pytest.raises()` to assert exceptions.
