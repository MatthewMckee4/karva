"""Support for providing temporary directories to test functions.

Adapted from pytest's ``_pytest/tmpdir.py`` (commit 8ecf49ec2). Only
``TempPathFactory`` and the ``get_user`` helper are included; pytest's
``Config`` integration, ``pytest_configure``/``pytest_sessionfinish`` hooks,
and the per-test ``tmp_path`` fixture are intentionally omitted because
karva does not yet expose a ``FixtureRequest`` object.

The following adaptations were made:

- ``from_config`` and the ``_ispytest``/``check_ispytest`` machinery are
  dropped; constructor takes plain arguments.
- ``_trace`` is optional and defaults to a no-op callable.
- ``RetentionType`` is kept; the default policy is ``"all"`` and the default
  retention count is ``3`` (matching pytest's defaults).

See the pytest license block in this repository's LICENSE file for the
applicable copyright notice.
"""

from __future__ import annotations

import dataclasses
import getpass
import os
import stat
import sys
import tempfile
from collections.abc import Callable
from pathlib import Path
from typing import Literal, final

if os.name == "nt":
    from pathlib import WindowsPath as PlatformPath
else:
    from pathlib import PosixPath as PlatformPath

from karva._fixtures.pathlib import (
    LOCK_TIMEOUT,
    make_numbered_dir,
    make_numbered_dir_with_cleanup,
    rm_rf,
)

RetentionType = Literal["all", "failed", "none"]


class LegacyPath(PlatformPath):
    """Path with the legacy methods exposed by pytest's ``tmpdir`` fixture."""

    @property
    def strpath(self) -> str:
        """Return the path as a string."""
        return str(self)

    def join(self, *parts: str | os.PathLike[str]) -> LegacyPath:
        """Return a path formed by appending path components."""
        return self.joinpath(*parts)

    def write(self, data: str) -> None:
        """Write UTF-8 text to the path."""
        self.write_text(data, encoding="utf-8")

    def read(self) -> str:
        """Read UTF-8 text from the path."""
        return self.read_text(encoding="utf-8")

    def write_binary(self, data: bytes) -> None:
        """Write bytes to the path."""
        self.write_bytes(data)

    def read_binary(self) -> bytes:
        """Read bytes from the path."""
        return self.read_bytes()


def _noop_trace(*_args: object, **_kwargs: object) -> None:
    """Default ``trace`` callback used when no tracer is supplied."""


@final
@dataclasses.dataclass
class TempPathFactory:
    """Factory for temporary directories under the common base temp directory.

    See pytest's :ref:`temporary directory location and retention` for the
    semantics this implementation follows.
    """

    _given_basetemp: Path | None
    _trace: Callable[..., object]
    _basetemp: Path | None
    _retention_count: int
    _retention_policy: RetentionType

    def __init__(
        self,
        given_basetemp: Path | None = None,
        retention_count: int = 3,
        retention_policy: RetentionType = "all",
        trace: Callable[..., object] = _noop_trace,
        basetemp: Path | None = None,
    ) -> None:
        if given_basetemp is None:
            self._given_basetemp = None
        else:
            # Use os.path.abspath() to get absolute path instead of resolve() as
            # it does not work the same in all platforms (see pytest #4427).
            self._given_basetemp = Path(os.path.abspath(str(given_basetemp)))
        self._trace = trace
        self._retention_count = retention_count
        self._retention_policy = retention_policy
        self._basetemp = basetemp

    def _ensure_relative_to_basetemp(self, basename: str) -> str:
        basename = os.path.normpath(basename)
        if (self.getbasetemp() / basename).resolve().parent != self.getbasetemp():
            raise ValueError(f"{basename} is not a normalized and relative path")
        return basename

    def mktemp(self, basename: str, numbered: bool = True) -> Path:
        """Create a new temporary directory managed by the factory.

        Args:
            basename: Directory base name, which must be a relative path.
            numbered: If true, ensure the directory is unique by adding a numbered
                suffix greater than any existing one: ``basename="foo-"`` and
                ``numbered=True`` creates directories named ``"foo-0"``,
                ``"foo-1"``, ``"foo-2"`` and so on.

        Returns:
            The new directory path.
        """
        basename = self._ensure_relative_to_basetemp(basename)
        if not numbered:
            p = self.getbasetemp().joinpath(basename)
            p.mkdir(mode=0o700)
        else:
            p = make_numbered_dir(root=self.getbasetemp(), prefix=basename, mode=0o700)
            self._trace("mktemp", p)
        return p

    def getbasetemp(self) -> Path:
        """Return the base temporary directory, creating it if needed.

        Returns:
            The base temporary directory.
        """
        if self._basetemp is not None:
            return self._basetemp

        if self._given_basetemp is not None:
            basetemp = self._given_basetemp
            if basetemp.exists():
                rm_rf(basetemp)
            basetemp.mkdir(mode=0o700)
            basetemp = basetemp.resolve()
        else:
            from_env = os.environ.get("KARVA_DEBUG_TEMPROOT")
            temproot = Path(from_env or tempfile.gettempdir()).resolve()
            user = get_user() or "unknown"
            # use a sub-directory in the temproot to speed-up
            # make_numbered_dir() call
            rootdir = temproot.joinpath(f"karva-of-{user}")
            try:
                rootdir.mkdir(mode=0o700, exist_ok=True)
            except OSError:
                rootdir = temproot.joinpath("karva-of-unknown")
                rootdir.mkdir(mode=0o700, exist_ok=True)
            # Because we use exist_ok=True with a predictable name, make sure
            # we are the owners, to prevent any funny business (on unix, where
            # temproot is usually shared). Also, to keep things private, fixup
            # any world-readable temp rootdir's permissions. Don't follow
            # symlinks, otherwise we're open to a symlink-swapping TOCTOU.
            uid = _get_user_id()
            if uid is not None:
                stat_follow_symlinks = os.stat not in os.supports_follow_symlinks
                rootdir_stat = rootdir.stat(follow_symlinks=stat_follow_symlinks)
                if stat.S_ISLNK(rootdir_stat.st_mode):
                    raise OSError(
                        f"The temporary directory {rootdir} is a symbolic link. "
                        "Fix this and try again."
                    )
                if rootdir_stat.st_uid != uid:
                    raise OSError(
                        f"The temporary directory {rootdir} is not owned by the current user. "
                        "Fix this and try again."
                    )
                if (rootdir_stat.st_mode & 0o077) != 0:
                    chmod_follow_symlinks = os.chmod not in os.supports_follow_symlinks
                    rootdir.chmod(
                        rootdir_stat.st_mode & ~0o077,
                        follow_symlinks=chmod_follow_symlinks,
                    )
            keep = self._retention_count
            if self._retention_policy == "none":
                keep = 0
            basetemp = make_numbered_dir_with_cleanup(
                prefix="karva-",
                root=rootdir,
                keep=keep,
                lock_timeout=LOCK_TIMEOUT,
                mode=0o700,
            )
        assert basetemp is not None, basetemp
        self._basetemp = basetemp
        self._trace("new basetemp", basetemp)
        return basetemp


def get_user() -> str | None:
    """Return the current user name when available.

    ``getuser()`` may fail in incomplete environments (see pytest #1010).
    """
    try:
        return getpass.getuser()
    except (OSError, KeyError):
        return None


def _get_user_id() -> int | None:
    """Return the current process's real user id or None if it could not be determined."""
    if sys.platform == "win32" or sys.platform == "emscripten":
        return None
    ERROR = -1
    uid = os.getuid()
    return uid if uid != ERROR else None
