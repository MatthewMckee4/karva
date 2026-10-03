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
The reason is retained in JSON, JSONL, and JUnit reports and explains an
unexpected pass. Expected failures still show `PASS` and count as passed in
the terminal.

## Conditions

Pass boolean conditions to expect failure when any condition is true:

```python title="test.py"
import sys
import karva

@karva.tags.expect_fail(sys.platform == "win32", reason="Known Windows bug")
def test_export():
    assert False  # Replace with an assertion for the known bug.
```

`@karva.tags.expect_fail(False, False)` runs as a normal test.

## Restricting the expected exception

Use `raises=` to require a particular exception or a tuple of exception classes.
Subclasses match normally:

```python
@karva.tags.expect_fail(reason="Known parser bug", raises=ValueError)
def test_invalid_input():
    parse("invalid")
```

A different exception is a normal failure and follows the retry policy. A matching
expected failure is never retried. Without `raises=`, ordinary test-body exceptions
are accepted. Fixture setup and teardown errors, missing fixtures, framework
timeouts, background exceptions, and non-`None` return values cannot become
expected failures.

## Pytest

`@pytest.mark.xfail` is also supported:

```python title="test.py"
import pytest

@pytest.mark.xfail(reason="Known bug")
def test_export():
    assert False  # Replace with an assertion for the known bug.
```
