# Project Non-Goals

Karva does not plan to support the features below.
Other pytest features may also be unsupported; absence from this list does
not mean support is planned.

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

## Pytest Plugin Compatibility

Karva does not support pytest plugins or hooks. Common plugin features may
be built into Karva directly. The [request fixture](fixtures/fixtures.md#the-request-fixture)
exposes Karva's collection nodes, configuration, and session.
