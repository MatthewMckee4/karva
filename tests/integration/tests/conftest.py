from karva import fixture

from .test_env import TestEnv


@fixture
def test_env() -> TestEnv:
    """Create a test environment for the entire test session."""
    return TestEnv()
