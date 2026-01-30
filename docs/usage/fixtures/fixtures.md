# Fixtures

Fixtures provide a mechanism for setup and teardown logic in tests. They enable dependency injection, allowing tests to declare their dependencies explicitly and receive them automatically.

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

Fixtures support different scopes that control their lifecycle and when they are created and destroyed.

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

The fixture is created once for the entire test session and shared across all tests:

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

## Dependent Fixtures

Fixtures can depend on other fixtures, enabling composition:

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

Example output when running with `karva test --show-output`:

```text
Creating database
Running test
Closing database
```

## Auto-Use Fixtures

Auto-use fixtures execute automatically for all tests within their scope, without requiring explicit declaration:

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

This is particularly useful for global setup and teardown:

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
