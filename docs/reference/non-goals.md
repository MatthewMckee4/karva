# Project Non-Goals

Karva is intentionally narrower than pytest. The goal is not to run every
pytest suite unchanged; the goal is a fast, explicit test runner with a smaller
surface area.

This page documents features Karva does not plan to support. If a feature is
not listed here, that does not mean it is planned.

## Class-Based Tests

Karva will not collect or run class-based tests.

Unsupported patterns include:

- `class TestSomething:` with `test_*` methods
- `unittest.TestCase` subclasses
- xUnit-style `setup_method`, `teardown_method`, `setup_class`, and
  `teardown_class`
- class-scoped fixture behavior

Use module-level test functions and fixtures instead:

```py title="test_service.py"
import karva


@karva.fixture
def service():
    return Service()


def test_service_handles_empty_input(service):
    assert service.handle("") == []
```

Classes are still fine as application code or local helpers inside tests. Only
classes as the test structure are out of scope.

## Pytest Request Internals

Karva will not expose pytest's `request` fixture or the `FixtureRequest` API.

Unsupported patterns include:

- `request.param`
- `request.getfixturevalue(...)`
- `request.addfinalizer(...)`
- `request.node`, `request.config`, and other pytest collection internals
- `pytestconfig`
- `pytest_generate_tests`

Use explicit fixture dependencies, generator fixtures, and normal configuration
inputs instead:

```py title="test_database.py"
import karva


@karva.fixture
def database():
    db = connect()
    yield db
    db.close()


def test_query(database):
    assert database.query("select 1") == 1
```

## Pytest Plugin Compatibility

Karva will not implement pytest's plugin or hook system as a compatibility
layer. Pytest plugins depend on pytest's collection tree, config object,
request object, and hook lifecycle, which are not part of Karva's design.

Karva can still provide first-party features inspired by common pytest plugins
when they fit the project. Those features should be designed as Karva features,
not as pytest plugin emulation.
