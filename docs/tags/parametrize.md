This works like pytest's `parametrize` decorator.

!!! note

    When mixing fixtures and parametrize, the parametrize values take priority.

First, here is a small example:

```python
import karva

@karva.tags.parametrize("a", [1, 2, 3])
def test_function(a: int):
    assert a > 0
```

Then running `uv run karva test` will provide the following output:

```bash
test test::test_function [a=1] ... ok
test test::test_function [a=2] ... ok
test test::test_function [a=3] ... ok

test result: ok. 3 passed; 0 failed; 0 skipped; finished in 0s

```

We can also parametrize multiple arguments:

```python
import karva

@karva.tags.parametrize(("a", "b"), [(1, 4), (2, 5), (3, 6)])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
```

Then running `uv run karva test` will provide the following output:

```bash
test test::test_function [a=1, b=4] ... ok
test test::test_function [a=2, b=5] ... ok
test test::test_function [a=3, b=6] ... ok

test result: ok. 3 passed; 0 failed; 0 skipped; finished in 0s

```

We can also mix fixtures and parametrize:

```python
import karva

@karva.fixture
def b() -> int:
    return 1

@karva.tags.parametrize("a", [1, 2])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
```

Then running `uv run karva test -v` will provide the following output:

```bash
test test::test_function [a=1, b=1] ... ok
test test::test_function [a=2, b=1] ... ok

test result: ok. 2 passed; 0 failed; 0 skipped; finished in 0s

```

We can also use multiple decorators:

This will result in a sort of cartesian product of the parametrize values.

```python
import karva

@karva.tags.parametrize("a", [1, 2])
@karva.tags.parametrize("b", [1, 2])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
```

Then running `uv run karva test -v` will provide the following output:

```bash
test test::test_function [a=1, b=1] ... ok
test test::test_function [a=2, b=1] ... ok
test test::test_function [a=1, b=2] ... ok
test test::test_function [a=2, b=2] ... ok

test result: ok. 4 passed; 0 failed; 0 skipped; finished in 0s

```

We can also still use pytest's `parametrize` decorator:

```python
import pytest

@pytest.mark.parametrize("a", [1, 2])
def test_function(a: int):
    assert a > 0
```

Then running `uv run karva test -v` will provide the following output:

```bash
test test::test_function [a=1] ... ok
test test::test_function [a=2] ... ok

test result: ok. 2 passed; 0 failed; 0 skipped; finished in 0s

```
