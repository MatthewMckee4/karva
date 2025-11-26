The `skip` tag allows us to mark test functions to be skipped during test execution.

When a test is skipped, it will not be run but will be counted in the test results.

## Basic Usage

```python title="test.py"
import karva

@karva.tags.skip
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... skipped

test result: ok. 0 passed; 0 failed; 1 skipped; finished in 0s
```

```python title="test.py"
import karva

@karva.tags.skip()
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... skipped

test result: ok. 0 passed; 0 failed; 1 skipped; finished in 0s
```

## Reason

You can provide a `str` reason as a positional or keyword argument.

```python title="test.py"
import karva

@karva.tags.skip("This test is not implemented yet")
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... skipped: Waiting for feature X to be implemented

test result: ok. 0 passed; 0 failed; 1 skipped; finished in 0s
```

```python title="test.py"
import karva

@karva.tags.skip(reason="Waiting for feature X to be implemented")
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... skipped: Waiting for feature X to be implemented

test result: ok. 0 passed; 0 failed; 1 skipped; finished in 0s
```

## Pytest

You can also still use `@pytest.mark.skip`.

```python title="test.py"
import pytest

@pytest.mark.skip(reason="Waiting for feature X to be implemented")
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... skipped: Waiting for feature X to be implemented

test result: ok. 0 passed; 0 failed; 1 skipped; finished in 0s
```

## Conditions

We can provide `bool` conditions as a positional arguments.

Then the test will only be skipped if all conditions are `True`.

```python title="test.py"
import karva

@karva.tags.skip(True)
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... skipped

test result: ok. 0 passed; 0 failed; 1 skipped; finished in 0s
```

You can still provide a reason as a keyword argument.

```python title="test.py"
import karva

@karva.tags.skip(True, reason="Waiting for feature X to be implemented")
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... skipped: Waiting for feature X to be implemented

test result: ok. 0 passed; 0 failed; 1 skipped; finished in 0s
```

### Multiple Conditions

```python title="test.py"
import karva

@karva.tags.skip(True, False) # This will not be skipped
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... FAILED

test result: ok. 0 passed; 1 failed; 0 skipped; finished in 0s
```

### Pytest

You can also still use `@pytest.mark.skipif`.

```python title="test.py"
import pytest

@pytest.mark.skipif(True, reason="Waiting for feature X to be implemented")
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... skipped: Waiting for feature X to be implemented

test result: ok. 0 passed; 0 failed; 1 skipped; finished in 0s
```
