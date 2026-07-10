from __future__ import annotations

import warnings
from dataclasses import dataclass


@dataclass(frozen=True)
class Calculator:
    offset: int = 0

    def add(self, left: int, right: int) -> int:
        return left + right + self.offset


def normalize_name(name: str) -> str:
    normalized = name.strip().lower()
    if not normalized:
        raise ValueError("name cannot be empty")
    return normalized


def legacy_label(name: str) -> str:
    warnings.warn("legacy_label is deprecated", DeprecationWarning, stacklevel=2)
    return f"legacy:{normalize_name(name)}"
