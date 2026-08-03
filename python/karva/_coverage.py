"""Bootstrap native coverage in opted-in Python child processes."""

from __future__ import annotations

import atexit
import json
import os
import uuid
import warnings
from collections.abc import Callable
from pathlib import Path
from typing import NoReturn, TypedDict, cast

from karva._karva import _ChildCoverageSession, _start_child_coverage

_CONFIG_ENV = "_KARVA_COVERAGE_CONFIG"
_collector: _ChildCoverageSession | None = None
_configured = False


class _Config(TypedDict):
    roots: list[str]
    directory: str
    branches: bool
    exclude_lines: list[str]
    partial_branches: list[str]
    context: str | None


def _load_config(raw: str) -> _Config:
    value = json.loads(raw)
    if not isinstance(value, dict):
        raise TypeError("Karva child coverage config must be an object")
    for name in ("roots", "exclude_lines", "partial_branches"):
        items = value.get(name)
        if not isinstance(items, list) or not all(
            isinstance(item, str) for item in items
        ):
            raise TypeError(f"Karva child coverage {name} must contain strings")
    if not isinstance(value.get("directory"), str):
        raise TypeError("Karva child coverage directory must be a string")
    if not isinstance(value.get("branches"), bool):
        raise TypeError("Karva child coverage branches must be a boolean")
    context = value.get("context")
    if context is not None and not isinstance(context, str):
        raise TypeError("Karva child coverage context must be a string or null")
    return cast(_Config, value)


def _start() -> None:
    global _collector
    raw = os.environ.get(_CONFIG_ENV)
    if raw is None:
        return
    config = _load_config(raw)
    data_file = Path(config["directory"]) / f"{os.getpid()}-{uuid.uuid4()}.json"
    _collector = _start_child_coverage(
        config["roots"],
        str(data_file),
        config["branches"],
        config["exclude_lines"],
        config["partial_branches"],
        config["context"],
    )


def _save() -> None:
    global _collector
    if _collector is None:
        return
    try:
        _collector.stop_and_save()
        _collector = None
    except Exception as error:
        warnings.warn(
            f"Karva failed to save child-process coverage: {error}", stacklevel=2
        )


def _exec_wrapper(original: Callable[..., NoReturn]) -> Callable[..., NoReturn]:
    def exec_with_coverage(*args: object, **kwargs: object) -> NoReturn:
        was_active = _collector is not None
        _save()
        try:
            original(*args, **kwargs)
        except BaseException:
            if was_active:
                _start()
            raise

    return exec_with_coverage


def _wrap_exit_and_exec() -> None:
    original_exit = os._exit

    def exit_with_coverage(status: int) -> NoReturn:
        _save()
        original_exit(status)

    setattr(os, "_exit", exit_with_coverage)  # noqa: B010
    for name in (
        "execl",
        "execle",
        "execlp",
        "execlpe",
        "execv",
        "execve",
        "execvp",
        "execvpe",
    ):
        original_value = getattr(os, name)
        if not callable(original_value):
            raise TypeError(f"os.{name} must be callable")
        setattr(
            os,
            name,
            _exec_wrapper(cast(Callable[..., NoReturn], original_value)),
        )


def _install_hooks() -> None:
    global _configured
    if _configured:
        return
    _configured = True
    atexit.register(_save)
    if hasattr(os, "register_at_fork"):
        os.register_at_fork(after_in_child=_start)
    _wrap_exit_and_exec()


def _configure(
    roots: list[str],
    directory: str,
    branches: bool,
    exclude_lines: list[str],
    partial_branches: list[str],
    context: str | None,
) -> None:
    os.environ[_CONFIG_ENV] = json.dumps(
        {
            "roots": roots,
            "directory": directory,
            "branches": branches,
            "exclude_lines": exclude_lines,
            "partial_branches": partial_branches,
            "context": context,
        },
        separators=(",", ":"),
    )
    bootstrap = str(Path(__file__).with_name("_coverage_bootstrap"))
    python_path = os.environ.get("PYTHONPATH")
    os.environ["PYTHONPATH"] = os.pathsep.join(
        [bootstrap, python_path] if python_path else [bootstrap]
    )
    _install_hooks()


def _bootstrap() -> None:
    if os.environ.get(_CONFIG_ENV) is None:
        return
    _start()
    _install_hooks()
