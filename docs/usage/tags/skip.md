# Skip

`@karva.tags.skip` skips a test and counts it in the results.

```python title="test.py"
import karva

@karva.tags.skip(reason="Not implemented yet")
def test_export():
    assert False
```

Use `@karva.tags.skip` or `@karva.tags.skip()` without a reason.
A reason can also be positional: `@karva.tags.skip("Not implemented yet")`.

## Conditions

Pass boolean conditions to skip a test only when all are true:

```python title="test.py"
import sys
import karva

@karva.tags.skip(sys.platform == "win32", reason="Requires Unix")
def test_unix_permissions():
    assert sys.platform != "win32"
```

`@karva.tags.skip(True, False)` does not skip the test.

## Running skipped tests

Run only tests with active skip conditions, or include them with other tests:

```bash
uv run karva test --run-ignored=only
uv run karva test --run-ignored=all
```

Their skip decorators are ignored, so they can pass or fail normally.
Tests with false skip conditions are not considered ignored.

## Pytest

`@pytest.mark.skip` and `@pytest.mark.skipif` are also supported:

```python title="test.py"
import sys
import pytest

@pytest.mark.skipif(sys.platform == "win32", reason="Requires Unix")
def test_unix_permissions():
    assert sys.platform != "win32"
```
