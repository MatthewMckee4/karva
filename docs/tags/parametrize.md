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

Then running `uv run karva test -v` will provide the following output:

```bash
INFO Discovering tests...
INFO Discovered 1 test in 1 file
INFO Running test: test_parametrize::test_function [1]
INFO Test test_parametrize::test_function [1] passed
INFO Running test: test_parametrize::test_function [2]
INFO Test test_parametrize::test_function [2] passed
INFO Running test: test_parametrize::test_function [3]
INFO Test test_parametrize::test_function [3] passed
Passed tests: 3
All checks passed!
```

We can also parametrize multiple arguments:

```python
import karva

@karva.tags.parametrize(("a", "b"), [(1, 4), (2, 5), (3, 6)])
def test_function(a: int, b: int):
    assert a > 0 and b > 0
```

Then running `uv run karva test -v` will provide the following output:

```bash
INFO Discovering tests...
INFO Discovered 1 test in 1 file
INFO Running test: test_parametrize::test_function [1, 4]
INFO Test test_parametrize::test_function [1, 4] passed
INFO Running test: test_parametrize::test_function [2, 5]
INFO Test test_parametrize::test_function [2, 5] passed
INFO Running test: test_parametrize::test_function [3, 6]
INFO Test test_parametrize::test_function [3, 6] passed
Passed tests: 3
All checks passed!
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
INFO Discovering tests...
INFO Discovered 1 test in 1 file
INFO Running test: test_parametrize::test_function [1, 1]
INFO Test test_parametrize::test_function [1, 1] passed
INFO Running test: test_parametrize::test_function [2, 1]
INFO Test test_parametrize::test_function [2, 1] passed
Passed tests: 2
All checks passed!
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
INFO Discovering tests...
INFO Discovered 1 test in 1 file
INFO Running test: test_parametrize::test_function [1, 1]
INFO Test test_parametrize::test_function [1, 1] passed
INFO Running test: test_parametrize::test_function [2, 1]
INFO Test test_parametrize::test_function [2, 1] passed
INFO Running test: test_parametrize::test_function [1, 2]
INFO Test test_parametrize::test_function [1, 2] passed
INFO Running test: test_parametrize::test_function [2, 2]
INFO Test test_parametrize::test_function [2, 2] passed
Passed tests: 4
All checks passed!
```

We can also still use pytest's `parametrize` decorator:

```python
import karva

@pytest.mark.parametrize("a", [1, 2])
def test_function(a: int):
    assert a > 0
```

Then running `uv run karva test -v` will provide the following output:

```bash
INFO Discovering tests...
INFO Discovered 1 test in 1 file
INFO Running test: test_parametrize::test_function [1]
INFO Test test_parametrize::test_function [1] passed
INFO Running test: test_parametrize::test_function [2]
INFO Test test_parametrize::test_function [2] passed
Passed tests: 2
All checks passed!
```
