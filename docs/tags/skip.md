# Skip Tag

The `skip` tag allows you to mark test functions to be skipped during test execution. When a test is skipped, it will not be run but will be counted in the test results.

## Basic Usage

**Skip without reason**

```python
import karva

@karva.tags.skip
def test_example():
    assert False  # This will not be executed
```

**Skip with function call**

```python
import karva

@karva.tags.skip()
def test_example():
    assert False  # This will not be executed
```

**Skip with reason (positional argument)**

```python
import karva

@karva.tags.skip("This test is not implemented yet")
def test_example():
    assert False  # This will not be executed
```

**Skip with reason (keyword argument)**

```python
import karva

@karva.tags.skip(reason="Waiting for feature X to be implemented")
def test_example():
    assert False  # This will not be executed
```

## Behavior

When a test is marked with the `skip` tag:

- The test function will not be executed
- The test will be counted as "skipped" in the test results
- If a reason is provided, it will be shown in the `info` logs (access via using `-v`)

## Use Cases

- **Temporarily disable tests**: When debugging or working on specific functionality
- **Mark incomplete tests**: Tests that are written but not yet ready to run
- **Platform-specific tests**: Tests that should only run on certain platforms
- **Feature flags**: Tests for features that are not yet enabled

## Example Output

When running tests with skipped test cases:

```bash
test result: ok. 2 passed; 0 failed; 1 skipped
```

The skipped tests contribute to the total count but do not cause the test suite to fail.
