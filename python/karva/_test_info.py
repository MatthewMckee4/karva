"""Read-only tag names for one concrete test case."""

from typing import NamedTuple


class TestInfo(NamedTuple):
    """Tag names resolved for a test, including inherited and parameter tags."""

    tags: frozenset[str]

    def has_tag(self, name: str) -> bool:
        """Return whether this test case has the given tag name."""
        return name in self.tags
