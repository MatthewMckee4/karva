The `expect_fail` tag is used to mark a test as expected to fail. When a test marked with `expect_fail` fails, it is considered a success. When a test marked with `expect_fail` passes, it is considered a failure.

## Basic Usage

```python title="test.py"
import karva

@karva.tags.expect_fail
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... ok

test result: ok. 1 passed; 0 failed; 0 skipped; finished in 0s
```

```python title="test.py"
import karva

@karva.tags.expect_fail()
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... ok

test result: ok. 1 passed; 0 failed; 0 skipped; finished in 0s
```

## Reason

You can provide a `str` reason as a positional or keyword argument.

```python title="test.py"
import karva

@karva.tags.expect_fail("This test is expected to fail")
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... ok

test result: ok. 1 passed; 0 failed; 0 skipped; finished in 0s
```

```python title="test.py"
import karva

@karva.tags.expect_fail(reason="This test is expected to fail")
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... ok

test result: ok. 1 passed; 0 failed; 0 skipped; finished in 0s
```

The reason only shows when the test passes when expected to fail.

```python title="test.py"
import karva

@karva.tags.expect_fail(reason="This test is expected to fail")
def test_function():
    assert True
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... FAILED

test failures:

test `test::test_function` at test.py:3 passed when it was expected to fail
reason: This test is expected to fail

test failures:
    test::test_function at test.py:3

test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in 0s
```

## Pytest

We can also still use `@pytest.mark.xfail`.

```python title="test.py"
import pytest

@pytest.mark.xfail(reason="This test is expected to fail")
def test_function():
    assert False
```

```text
test test::test_function ... ok

test result: ok. 1 passed; 0 failed; 0 skipped; finished in 0s
```

## Conditions

We can provide `bool` conditions as a positional arguments.

Then the test will only be expected to fail if all conditions are `True`.

```python title="test.py"
import karva

@karva.tags.expect_fail(True)
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... ok

test result: ok. 1 passed; 0 failed; 0 skipped; finished in 0s
```

You can still provide a reason as a keyword argument.

```python title="test.py"
import karva

@karva.tags.expect_fail(True, reason="Waiting for feature X to be implemented")
def test_function():
    assert False
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... ok

test result: ok. 1 passed; 0 failed; 0 skipped; finished in 0s
```

### Multiple Conditions

```python title="test.py"
import karva

@karva.tags.expect_fail(True, False)
def test_function():
    assert True
```

Then running `uv run karva test` will provide the following output:

```text
test test::test_function ... FAILED

test failures:

test `test::test_function` at test.py:3 passed when it was expected to fail

test failures:
    test::test_function at test.py:3

test result: FAILED. 0 passed; 1 failed; 0 skipped; finished in 0s
```
