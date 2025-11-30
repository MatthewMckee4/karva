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
