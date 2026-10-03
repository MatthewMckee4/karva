The `expect_fail` tag is used to mark a test as expected to fail. When a test marked with `expect_fail` fails, Karva reports `PASS` and counts the result as passed in the terminal. JSON, JSONL, and JUnit reports retain the expected-failure outcome. When a test marked with `expect_fail` passes, it is considered a failure.

## Basic Usage

```python title="test.py"
import karva

@karva.tags.expect_fail
def test_function():
    assert False
```

Then running `uv run karva test` reports one passed test.

```python title="test.py"
import karva

@karva.tags.expect_fail()
def test_function():
    assert False
```

Then running `uv run karva test` reports one passed test.

## Reason

You can provide a `str` reason as a positional or keyword argument.

```python title="test.py"
import karva

@karva.tags.expect_fail("This test is expected to fail")
def test_function():
    assert False
```

Then running `uv run karva test` reports one passed test.

```python title="test.py"
import karva

@karva.tags.expect_fail(reason="This test is expected to fail")
def test_function():
    assert False
```

Then running `uv run karva test` reports one passed test.

The reason is retained in JSON, JSONL, and JUnit reports. It also explains an unexpected pass.

```python title="test.py"
import karva

@karva.tags.expect_fail(reason="This test is expected to fail")
def test_function():
    assert True
```

Then running `uv run karva test` will result in one failed test.

## Pytest

We can also still use `@pytest.mark.xfail`.

```python title="test.py"
import pytest

@pytest.mark.xfail(reason="This test is expected to fail")
def test_function():
    assert False
```

Then running `uv run karva test` reports one passed test.

## Conditions

We can provide `bool` conditions as a positional arguments.

Then the test will only be expected to fail if all conditions are `True`.

```python title="test.py"
import karva

@karva.tags.expect_fail(True)
def test_function():
    assert False
```

Then running `uv run karva test` reports one passed test.

You can still provide a reason as a keyword argument.

```python title="test.py"
import karva

@karva.tags.expect_fail(True, reason="Waiting for feature X to be implemented")
def test_function():
    assert False
```

Then running `uv run karva test` reports one passed test.

### Multiple Conditions

```python title="test.py"
import karva

@karva.tags.expect_fail(True, False)
def test_function():
    assert True
```

Then running `uv run karva test` will result in one failed test.
