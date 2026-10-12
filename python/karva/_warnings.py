"""Warning policy and isolated capture windows used by the embedded worker."""

from __future__ import annotations

import builtins
import importlib
import re
import sys
import warnings
from typing import Any, Literal

Action = Literal["default", "error", "ignore", "always", "module", "once"]
ACTIONS: tuple[Action, ...] = ("default", "error", "ignore", "always", "module", "once")


def parse_filter(value: str) -> tuple[Action, str, type[Warning], str, int]:
    """Validate Python's colon format, with regex message and module fields.

    Unlike interpreter ``-W`` parsing, invalid policy is an error. Later filters
    take precedence, and categories can be built-ins or importable dotted names.
    """
    parts = value.split(":")
    if len(parts) > 5:
        raise ValueError(f"invalid warning filter {value!r}: too many fields")
    parts.extend([""] * (5 - len(parts)))
    action, message, category_name, module, line = (part.strip() for part in parts)
    action = action or "default"
    if action == "all":
        action = "always"
    resolved_action = next((name for name in ACTIONS if name.startswith(action)), None)
    if resolved_action is None:
        raise ValueError(f"invalid warning filter {value!r}: unknown action {action!r}")
    category: Any = Warning
    if category_name:
        module_name, _, name = category_name.rpartition(".")
        try:
            namespace = (
                importlib.import_module(module_name) if module_name else builtins
            )
            category = getattr(namespace, name)
        except (ImportError, AttributeError) as error:
            raise ValueError(
                f"invalid warning filter {value!r}: unknown category {category_name!r}"
            ) from error
        if not isinstance(category, type) or not issubclass(category, Warning):
            raise ValueError(
                f"invalid warning filter {value!r}: {category_name!r} is not a warning category"
            )
    try:
        lineno = int(line or "0")
        if lineno < 0:
            raise ValueError("line must be non-negative")
        re.compile(message, re.IGNORECASE)
        re.compile(module)
    except (ValueError, re.error) as error:
        raise ValueError(f"invalid warning filter {value!r}: {error}") from error
    return resolved_action, message, category, module, lineno


def validate_filters(filters: list[str]) -> None:
    """Reject malformed policy before collection or fixture execution."""
    for value in filters:
        parse_filter(value)


class WarningCapture:
    """Record unconsumed warnings and restore filters and suppression registries.

    Nested user recording contexts own their warnings. A fresh registry for each
    attempt makes counts independent of worker assignment, parameter order, and
    retries. Modules imported within the window lose only their warning registry.
    """

    def __init__(self, filters: list[str]) -> None:
        self.filters = filters
        self.context = warnings.catch_warnings(record=True)
        self.registries: list[tuple[dict[str, Any], Any, dict[Any, Any]]] = []
        self.once_registry: dict[Any, Any] = vars(warnings)["onceregistry"]
        self.once = self.once_registry.copy()

    def __enter__(self) -> None:
        self.records = self.context.__enter__()
        for module in tuple(sys.modules.values()):
            namespace = getattr(module, "__dict__", None)
            if isinstance(namespace, dict):
                registry = namespace.get("__warningregistry__")
                if isinstance(registry, dict):
                    self.registries.append((namespace, registry, registry.copy()))
                    registry.clear()
        self.once_registry.clear()
        try:
            warnings.simplefilter("default")
            for value in self.filters:
                warnings.filterwarnings(*parse_filter(value))
        except BaseException:
            self.finish()
            raise

    def finish(self) -> list[tuple[str, str, str, int]]:
        """Close capture before formatting records, even if user state is broken."""
        self.context.__exit__(None, None, None)
        for module in tuple(sys.modules.values()):
            namespace = getattr(module, "__dict__", None)
            if isinstance(namespace, dict):
                namespace.pop("__warningregistry__", None)
        for namespace, original, saved in self.registries:
            original.clear()
            original.update(saved)
            namespace["__warningregistry__"] = original
        self.once_registry.clear()
        self.once_registry.update(self.once)
        vars(warnings)["onceregistry"] = self.once_registry
        return [
            (
                str(record.message),
                record.category.__name__,
                record.filename,
                record.lineno,
            )
            for record in self.records
        ]
