## Constraints

We only discover package level fixtures from a `conftest.py` file in the root of that package.

## Fixture Types

Karva supports different types of fixtures based on their scope:

### Function Scope

The default scope. The fixture is created for each test function.

### Module Scope

The fixture is created once per test module.

### Package Scope

The fixture is created once per test package.

This means that every package that is a sub-package of the package with the fixture will create the fixture if and only if it has a module that requires the fixture.

For example, if we have the following structure:

```
calculator
├── src
└── tests
    ├── conftest.py
    ├── bar
    │   └── test_bar.py
    └── foo
        └── test_foo.py
```

If we have the following fixture in `tests/conftest.py`:

```py
@fixture(scope="package")
def package_fixture():
    return "package"
```

And if the fixtures is used in `tests/foo/test_foo.py` and `tests/bar/test_bar.py`
then fixture will be created once for the `foo` package and once for the `bar` package.

If you wanted it to only create the fixture once you should use the `session` scope.

### Session Scope

The fixture is created once per test session.

### Dynamic Scope

A dynamic scope is given as a function that returns a valid scope string.

```py
def dynamic_scope(fixture_name, config):
    return "module"
```

Currently, we do not support config and that value is passed as `None`. The `fixture_name` argument is a string.

## Dependent fixtures

We support fixtures that depend on other fixtures.

```py
from karva import fixture

@fixture
def function_fixture():
    return "function"

@fixture
def dependent_fixture(function_fixture: str) -> str:
    return function_fixture + "dependent"

def test_dependent(dependent_fixture: str):
    assert dependent_fixture == "functiondependent"
```

## Finalizers

We support finalizers. These are called after the scope of the fixture has finished running.

```py
from karva import fixture

@fixture
def finalizer_fixture():
    print("Finalizer fixture initialized")
    yield 1
    print("Finalizer fixture finalizer called")

def test_finalizer(finalizer_fixture: int):
    assert finalizer_fixture == 1
```

## Auto-use fixtures

We support auto-use fixtures. These are fixtures that are automatically used in their scope.

```py
from karva import fixture

data = {}

@fixture(auto_use=True)
def add_data():
    data.update(value=True)

def test_value():
    assert data.get('value')
```

## Other ways to use fixtures

Seen [here](https://docs.pytest.org/en/7.1.x/how-to/fixtures.html#use-fixtures-in-classes-and-modules-with-usefixtures) in pytest.

You can wrap your test function with a decorator specifying what fixtures you would like to call before running the function.

This is technically a tag, but we reference it here has it refers to fixtures.

```py
import karva

@karva.fixture
def x():
    # Do something
    return ...

@karva.fixture
def y():
    # Do something
    yield ...
    # Do something else

@karva.tags.use_fixtures("x", "y")
def test():
    # Do something
```

## Overriding fixtures

We can _override_ fixtures by giving them the same name. When overriding them we can also use them as arguments.

**conftest.py**

```py
import pytest

@pytest.fixture
def username():
    return 'username'

```

**test_something.py**

```py
def test_username(username):
    assert username == 'username'
```

**subfolder/conftest.py**

```py
import pytest

@pytest.fixture
def username(username):
    return 'overridden-' + username
```

**subfolder/test_something_else.py**

```py
def test_username(username):
    assert username == 'overridden-username'
```

## Example

```bash
uv init --lib calculator
```

This will give us a project that looks like this:

```
calculator
├── pyproject.toml
├── README.md
└── src
    └── calculator
        ├── __init__.py
        └── py.typed
```

We can then create our core logic in `src/calculator/__init__.py`.

```py
class Calculator:
    def add(self, a: int, b: int) -> int:
        return a + b
```

We can then create our fixtures in `tests/conftest.py`.

```py
from karva import fixture

@fixture
def calculator() -> Calculator:
    return Calculator()
```

We can then create our tests in `tests/test_add.py`.

```py
from calculator import Calculator

def test_add(calculator: Calculator):
    assert calculator.add(1, 2) == 3
```

Then, we'll add karva to our project.

```bash
uv add --dev karva
```

We can then run our tests with `uv run karva test`.

```bash
uv run karva test
```
