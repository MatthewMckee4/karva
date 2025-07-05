from collections.abc import Generator

from karva import fixture

from .test_env import TestEnv


@fixture
def test_env() -> Generator[TestEnv, None, None]:
    """Create a test environment for the entire test session."""
    test_env = TestEnv()
    yield test_env
