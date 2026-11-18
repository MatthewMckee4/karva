import builtins
from typing import Any
from unittest.mock import (
    ANY as ANY,
)
from unittest.mock import (
    DEFAULT as DEFAULT,
)
from unittest.mock import (
    AsyncMock as AsyncMock,
)
from unittest.mock import (
    MagicMock as MagicMock,
)
from unittest.mock import (
    Mock as Mock,
)
from unittest.mock import (
    PropertyMock as PropertyMock,
)
from unittest.mock import (
    call as call,
)
from unittest.mock import (
    mock_open as mock_open,
)
from unittest.mock import (
    seal as seal,
)
from unittest.mock import (
    sentinel as sentinel,
)

class _Patcher:
    """The unittest.mock.patch APIs, started and owned by the fixture."""

    def __call__(self, target: str, *args: Any, **kwargs: Any) -> Any: ...
    def object(
        self, target: builtins.object, attribute: str, *args: Any, **kwargs: Any
    ) -> Any: ...
    context_manager = object
    def dict(self, in_dict: Any, *args: Any, **kwargs: Any) -> Any: ...
    def multiple(
        self, target: Any, *args: Any, **kwargs: Any
    ) -> builtins.dict[str, Any]: ...

class MockerFixture:
    """Function-scoped mock helpers; patches are restored after each attempt."""

    Mock = Mock
    MagicMock = MagicMock
    AsyncMock = AsyncMock
    PropertyMock = PropertyMock
    ANY = ANY
    DEFAULT = DEFAULT
    call = call
    sentinel = sentinel
    mock_open = staticmethod(mock_open)
    seal = staticmethod(seal)
    patch: _Patcher
    def __init__(self) -> None: ...
    def create_autospec(self, spec: Any, *args: Any, **kwargs: Any) -> Any: ...
    def spy(self, obj: object, name: str, *, duplicate_iterators: bool = False) -> Any:
        """Call the original and record results and exceptions on the spy."""
    def stub(self, name: str | None = None) -> MagicMock: ...
    def async_stub(self, name: str | None = None) -> AsyncMock: ...
    def stop(self, mock: Any) -> None:
        """Stop the active patch that returned mock; raise ValueError if unowned."""
    def stopall(self) -> None:
        """Restore all patches owned by this fixture in reverse order."""
    def resetall(
        self, *, return_value: bool = False, side_effect: bool = False
    ) -> None:
        """Reset patches and spies, excluding direct constructors and stubs."""
