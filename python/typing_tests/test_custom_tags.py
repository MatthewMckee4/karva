from typing import assert_type

import karva


@karva.tags.smoke
def smoke_test(value: int) -> str:
    """Keep the decorated callable's signature."""
    return str(value)


@karva.tags.integration("database", owner="checkout")
def integration_test(value: int) -> str:
    """Keep signatures for custom tags called with metadata."""
    return str(value)


assert_type(smoke_test(1), str)
assert_type(integration_test(1), str)
