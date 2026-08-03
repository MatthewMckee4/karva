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

## Pytest Plugin Compatibility

Karva will not implement pytest's plugin or hook system as a compatibility
layer. This includes `pytestconfig`, `pytest_generate_tests`, and plugins that
depend on pytest's collection tree or hook lifecycle.

Karva can still provide first-party features inspired by common pytest plugins
when they fit the project. Those features should be designed as Karva features,
not as pytest plugin emulation.
