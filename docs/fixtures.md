## Constraints

We only discover package level fixtures from a `conftest.py` file in the root of that package.

## Fixture Types

Karva supports different types of fixtures based on their scope:

### Function Scope

The default scope. The fixture is created for each test function.

### Module Scope

The fixture is created once per test module.

### Package Scope

The fixture is created once per folder.

This means that every package that is a sub-package of the package with the fixture will create the fixture if and only if it has a module that requires the fixture.

For example, if we have the following structure:

```text
calculator
├── src
└── tests
    ├── conftest.py
    ├── bar
    │   └── test_bar.py
    └── foo
        └── test_foo.py
```

And the following fixture:

```py title="tests/conftest.py"
@fixture(scope="package")
def package_fixture():
    return "package"
```

If the fixture is used in `tests/foo/test_foo.py` and `tests/bar/test_bar.py`
then fixture will be created once for the `foo` folder and once for the `bar` folder.

If you wanted it to only create the fixture once you should use the `session` scope.

### Session Scope

The fixture is created once per test session.

### Dynamic Scope

A dynamic scope is given as a function that returns a valid scope string.

```py
def dynamic_scope(fixture_name: str, config: object) -> str:
    return "module"
```

Currently, we do not support config and that value is passed as `None`. The `fixture_name` argument is a string.

## Dependent fixtures

Fixture are allowed to depend on other fixtures, and so on.

```py
from karva import fixture

@fixture
def function_fixture() -> str:
    return "fixture"

@fixture
def dependent_fixture(function_fixture: str) -> str:
    return "dependent_" + function_fixture

def test_dependent(dependent_fixture: str):
    assert dependent_fixture == "dependent_fixture"
```

## Finalizers

Finalizers are called after the scope of the fixture has finished running.

Here we do some setup for the fixture, then yield the value. Once the requesting function, here `test_finalizer`, is done, we run the finalizer, and the teardown logic is run.

This can be useful for setting up a database, then deleting it after the test is done.

```py
from karva import fixture

@fixture
def finalizer_fixture() -> Iterator[int]:
    print("setup")
    yield 1
    print("teardown")

def test_finalizer(finalizer_fixture: int) -> None:
    print("running test")
    assert finalizer_fixture == 1
```

If we ran `karva test -s` here, we would see the following output:

```text
setup
running test
teardown
```

## Auto-use fixtures

Auto-use fixtures are run before any functions in their scope.

```py
from karva import fixture

data = {}

@fixture(auto_use=True)
def add_data():
    data.update(value=True)

def test_value():
    assert data.get('value')
```

These can be useful with finalizers, since the requesting function may not need any value.

```py
from karva import fixture

@fixture(auto_use=True)
def setup_db():
    print("setup")
    yield
    print("teardown")

def test_db():
    print("running test")
```

## Use-fixtures

We can use the `use_fixtures` tag to specify fixtures that should be run before a function.

This is useful when we don't need a value from the fixture, but we want to run some code before the test.

```py
import karva

@karva.fixture
def x():
    # Do something


@karva.fixture
def y():
    # Do something
    yield
    # Do something else

@karva.tags.use_fixtures("x", "y")
def test():
    # Do something
```

## Overriding fixtures

We can _override_ fixtures by giving them the same name. When overriding a fixture, we can still use the parent fixture.

```py title="conftest.py"
import pytest

@pytest.fixture
def username() -> str:
    return 'username'

```

```py title="test.py"
def test_username(username: str) -> None:
    assert username == 'username'
```

```py title="foo/conftest.py"
import pytest

@pytest.fixture
def username(username: str) -> str:
    return 'overridden-' + username
```

```py title="foo/test.py"
def test_username(username: str) -> None:
    assert username == 'overridden-username'
```

## Parametrizing fixtures

You can parametrize fixtures allowing us to run a test multiple times for each param of the fixture.

```py
import karva

@karva.fixture(params=['username', 'email'])
def some_fixture(request) -> str:
    return request.param

def test_username_email(some_fixture: str):
    assert some_fixture in ['username', 'email']
```

This will run `test_username_email` twice, once with `username` and once with `email`.

Here we also see that we can "introspect" the fixture by using the `request` object.

Currently the only parameter you can use here is `request.param`.

In future you will be able to access other parameters.

It is important to note that this request object is not the same as the pytest `FixtureRequest` object. It is a custom object provided by Karva.
And so it may not have all of the information that the pytest `FixtureRequest` object has.

## Built-in fixtures

Karva provides a few built-in fixtures that can be used in your tests.

We will try to add more built-in fixtures from pytest in the future.

### Temporary Directory

This fixture provides the user with a `pathlib.Path` object that points to a temporary directory.

You can use any of the following fixture names to use this fixture:

- `tmp_path` (from pytest)
- `tmpdir` (from pytest)
- `temp_path` (from karva)
- `temp_dir` (from karva)

### Monkeypatch

The `monkeypatch` fixture allows you to safely modify objects, dictionaries, environment variables, and the system path during tests. All changes are automatically undone after the test completes.

This fixture is compatible with pytest's `monkeypatch` fixture.

```py
def test_setattr(monkeypatch):
    import os
    monkeypatch.setattr(os, 'getcwd', lambda: '/fake/path')
    assert os.getcwd() == '/fake/path'

def test_setenv(monkeypatch):
    monkeypatch.setenv('MY_VAR', 'test_value')
    import os
    assert os.environ['MY_VAR'] == 'test_value'
```
