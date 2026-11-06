"""Karva is a Python test framework, written in Rust."""

from importlib import import_module
from typing import TYPE_CHECKING

from karva._approx import approx
from karva._builtins import MockEnv
from karva._fixtures.recwarn import deprecated_call, warns
from karva._importorskip import importorskip
from karva._karva import (
    Command,
    ExceptionInfo,
    FailError,
    RaisesContext,
    SkipError,
    SnapshotMismatchError,
    SnapshotSettings,
    assert_cmd_snapshot,
    assert_json_snapshot,
    assert_snapshot,
    fail,
    fixture,
    karva_run,
    param,
    raises,
    skip,
    snapshot_settings,
    tags,
)

__version__ = "0.0.1-alpha.13"

if TYPE_CHECKING:
    from karva._benchmark import Benchmark


def __getattr__(name: str) -> object:
    if name == "Benchmark":
        benchmark = import_module("karva._benchmark").Benchmark
        globals()[name] = benchmark
        return benchmark
    raise AttributeError(f"module 'karva' has no attribute {name!r}")


def __dir__() -> list[str]:
    """Include lazy exports without importing their implementation."""
    return sorted({*globals(), "Benchmark"})


__all__: list[str] = [
    "Benchmark",
    "Command",
    "ExceptionInfo",
    "FailError",
    "MockEnv",
    "RaisesContext",
    "SkipError",
    "SnapshotMismatchError",
    "SnapshotSettings",
    "approx",
    "assert_cmd_snapshot",
    "assert_json_snapshot",
    "assert_snapshot",
    "deprecated_call",
    "fail",
    "fixture",
    "importorskip",
    "karva_run",
    "param",
    "raises",
    "skip",
    "snapshot_settings",
    "tags",
    "warns",
]
