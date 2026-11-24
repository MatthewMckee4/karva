# Fixtures

Fixtures share setup and teardown. Tests request them by argument name.

## Defining Fixtures

Define a fixture using the `@karva.fixture` decorator:

```py title="test.py"
import karva


@karva.fixture
def database_connection():
    return create_connection()


def test_query(database_connection):
    result = database_connection.execute("SELECT 1")
    assert result == 1
```

## Fixture Scopes

Scope controls how long a fixture is shared within each worker.

### Function Scope (Default)

The fixture is created and destroyed for each test function that uses it:

```py title="test.py"
import karva


@karva.fixture
def counter():
    return {"count": 0}


def test_first(counter):
    counter["count"] += 1
    assert counter["count"] == 1


def test_second(counter):
    # Fresh instance, count is 0 again
    assert counter["count"] == 0
```

### Module Scope

The fixture is created once per test module and shared across all tests in that module:

```py title="test.py"
import karva


@karva.fixture(scope="module")
def shared_resource():
    print("Creating resource")
    return ExpensiveResource()
```

### Package Scope

The fixture is created once per package (directory). Each sub-package that contains tests using the fixture receives its own instance.

Consider the following directory structure:

```text
tests/
├── conftest.py
├── unit/
│   └── test_unit.py
└── integration/
    └── test_integration.py
```

```py title="tests/conftest.py"
import karva


@karva.fixture(scope="package")
def package_resource():
    return create_resource()
```

If both `test_unit.py` and `test_integration.py` use `package_resource`, the fixture is instantiated once for `unit/` and once for `integration/`.

### Session Scope

The fixture is created once per worker and shared across its tests:

```py title="conftest.py"
import karva


@karva.fixture(scope="session")
def global_config():
    return load_configuration()
```

### Dynamic Scope

The scope can be determined at runtime using a callable:

```py title="conftest.py"
import karva
import os


def determine_scope(fixture_name: str, config: object) -> str:
    if os.environ.get("CI"):
        return "session"
    return "function"


@karva.fixture(scope=determine_scope)
def adaptive_fixture():
    return create_resource()
```

The callable receives `fixture_name` as a string and `config` (currently `None`).

## The request fixture

Add a `request` argument to a test or fixture to receive a
`karva.FixtureRequest`. Each fixture gets a request for its own lifetime.
A test receives a function-scoped request with `fixturename` set to `None`.

```py title="test.py"
import karva


@karva.fixture
def log_path(request: karva.FixtureRequest):
    directory = request.getfixturevalue("tmp_path")
    path = directory / "events.log"
    request.addfinalizer(lambda: path.unlink(missing_ok=True))
    return path


def test_log(log_path):
    log_path.write_text("ok")
    assert log_path.read_text() == "ok"
```

`getfixturevalue(name)` uses the same scope and cache rules as a fixture
argument. During teardown, it can retrieve fixtures that are still active.
Karva raises `karva.FixtureLookupError` when lookup fails. Use
`request.raiseerror(message)` to raise the same exception with your own message.

`addfinalizer(callback)` runs callbacks in reverse registration order when
the fixture or test ends. Registered callbacks run even if setup fails.
For a generator fixture, Karva first resumes the generator after its yield.

`request.node` refers to the current test, module, package, or session.
`request.applymarker(marker)` adds a marker to that node. Pass a marker name or
a `pytest.mark` decorator. Use `node.get_closest_marker(name)` to look up a marker
on the node or its parents.

| Property | Value |
| --- | --- |
| `fixturename`, `fixturenames` | The current fixture name and the active fixture names. |
| `scope` | `"function"`, `"module"`, `"package"`, or `"session"`. |
| `node`, `keywords` | The node for the current scope and its keyword mapping. |
| `config`, `session` | Karva's worker configuration and session node. The config exposes `rootpath`, `settings`, and `getoption(name, default)`. |
| `function`, `cls`, `instance` | The test callable in function scope. `cls` is `None` in function scope, and `instance` is always `None`. |
| `module`, `path` | The test's Python module in function or module scope, and its file path in function, module, or package scope. |
| `param` | The value supplied by fixture or indirect parametrization. The attribute exists only when a value was supplied. |

### Parametrized fixtures

Pass `params` to the fixture decorator and read each value through
`request.param`. Karva runs every test that uses the fixture for each value.
Use `ids` to give the cases readable names.

```py title="test.py"
import karva


@karva.fixture(params=["utf-8", "ascii"])
def encoding(request: karva.FixtureRequest):
    return request.param


def test_encode(encoding):
    assert "hello".encode(encoding) == b"hello"
```

Use [indirect parametrization](../tags/parametrize.md#indirect-parametrization)
to choose the fixture's values for one test.

## Dependent Fixtures

Fixtures request other fixtures by argument name:

```py title="conftest.py"
import karva


@karva.fixture
def base_url():
    return "https://api.example.com"


@karva.fixture
def api_client(base_url):
    return APIClient(base_url)


@karva.fixture
def authenticated_client(api_client):
    api_client.authenticate()
    return api_client
```

```py title="test.py"
def test_api_call(authenticated_client):
    response = authenticated_client.get("/users")
    assert response.status_code == 200
```

## Teardown with Generators

Use generator fixtures to implement teardown logic. Code after `yield` executes after the fixture's scope ends:

```py title="conftest.py"
import karva
import shutil


@karva.fixture
def database():
    db = create_database()
    yield db
    db.close()


@karva.fixture(scope="module")
def temp_directory():
    path = create_temp_dir()
    yield path
    shutil.rmtree(path)
```

## Auto-Use Fixtures

Auto-use fixtures run for tests in their scope without being requested:

```py title="conftest.py"
import karva
import logging


@karva.fixture(auto_use=True)
def setup_logging():
    logging.basicConfig(level=logging.DEBUG)
    yield
    logging.shutdown()
```

```py title="test.py"
def test_something():
    # setup_logging runs automatically before this test
    pass
```

For session setup and teardown:

```py title="conftest.py"
import karva


@karva.fixture(scope="session", auto_use=True)
def database_migrations():
    run_migrations()
    yield
    rollback_migrations()
```

## Use-Fixtures Tag

The `use_fixtures` tag explicitly declares fixture dependencies when no return value is needed:

```py title="conftest.py"
import karva


@karva.fixture
def setup_cache():
    initialize_cache()
    yield
    clear_cache()


@karva.fixture
def seed_data():
    insert_test_data()
```

```py title="test.py"
import karva


@karva.tags.use_fixtures("setup_cache", "seed_data")
def test_cached_query():
    result = query_with_cache()
    assert result is not None
```

## Overriding Fixtures

Fixtures can be overridden in nested directories. The overriding fixture can reference the parent fixture:

```py title="conftest.py"
import karva


@karva.fixture
def username():
    return "default_user"
```

```py title="test_default.py"
def test_default_user(username):
    assert username == "default_user"
```

```py title="admin/conftest.py"
import karva


@karva.fixture
def username(username):
    return f"admin_{username}"
```

```py title="admin/test_admin.py"
def test_admin_user(username):
    assert username == "admin_default_user"
```

## Limitations

See [the request fixture](#the-request-fixture) for contextual properties and methods.
