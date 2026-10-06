# Expected failures

`@karva.tags.expect_fail` treats a failing test as a success and a passing
test as a failure.

```python title="test.py"
import karva

@karva.tags.expect_fail(reason="Waiting for the export fix")
def test_export():
    assert False  # Replace with an assertion for the known bug.
```

Use `@karva.tags.expect_fail` or `@karva.tags.expect_fail()` without a reason.
A reason can also be positional: `@karva.tags.expect_fail("Known bug")`.
The reason is shown only when the test unexpectedly passes.

## Conditions

Pass boolean conditions to expect failure only when all are true:

```python title="test.py"
import sys
import karva

@karva.tags.expect_fail(sys.platform == "win32", reason="Known Windows bug")
def test_export():
    assert False  # Replace with an assertion for the known bug.
```

`@karva.tags.expect_fail(True, False)` runs as a normal test.

## Pytest

`@pytest.mark.xfail` is also supported:

```python title="test.py"
import pytest

@pytest.mark.xfail(reason="Known bug")
def test_export():
    assert False  # Replace with an assertion for the known bug.
```
