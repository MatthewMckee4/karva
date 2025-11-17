from collections.abc import Sequence

from karva._karva import Tags

def parametrize(
    arg_names: Sequence[str] | str,
    arg_values: Sequence[Sequence[object]] | Sequence[object],
) -> Tags:
    """Parametrize the current test with the given arguments."""

def use_fixtures(*fixture_names: str) -> Tags:
    """Use the given fixtures for the current test.

    This is useful when you dont need the actual fixture
    but you need them to be called.
    """

def skip(*conditions: bool, reason: str | None = ...) -> Tags:
    """Skip the current test given the conditions."""

def expect_fail(*conditions: bool, reason: str | None = ...) -> Tags:
    """Expect the current test to fail given the conditions."""
