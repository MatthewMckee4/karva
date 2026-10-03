"""Import optional dependencies and skip tests when they are unavailable."""

from importlib import import_module
from types import ModuleType

from karva._karva import skip


def importorskip(
    modname: str,
    reason: str | None = None,
    *,
    exc_type: type[ImportError] | None = None,
) -> ModuleType:
    """Import and return a module, or skip when it cannot be imported.

    Missing modules skip with ``ModuleNotFoundError`` by default. Pass
    ``exc_type=ImportError`` to also skip import errors raised by an installed
    module.
    """
    _validate_module_name(modname)
    _validate_reason(reason)
    if exc_type is None:
        exc_type = ModuleNotFoundError
    elif not isinstance(exc_type, type) or not issubclass(exc_type, ImportError):
        raise TypeError("exc_type must be ImportError or a subclass")

    try:
        module = import_module(modname)
    except exc_type as error:
        if exc_type is ModuleNotFoundError and (
            not isinstance(error.name, str)
            or not (modname == error.name or modname.startswith(f"{error.name}."))
        ):
            raise
        skip(reason if reason is not None else f"could not import {modname!r}: {error}")

    return module


def _validate_module_name(modname: object) -> None:
    if not isinstance(modname, str):
        raise TypeError("modname must be a string")
    if not modname or any(not part.isidentifier() for part in modname.split(".")):
        raise ValueError(f"invalid module name {modname!r}")


def _validate_reason(reason: object) -> None:
    if reason is not None and not isinstance(reason, str):
        raise TypeError("reason must be a string or None")
