"""Path / filesystem helpers required by ``TempPathFactory``.

Adapted from pytest's ``_pytest/pathlib.py`` (commit 8ecf49ec2). Only the
helpers transitively required by ``make_numbered_dir_with_cleanup`` and
``cleanup_dead_symlinks`` are included; the rest of pytest's ``pathlib`` module
is not.

The following adaptations were made:

- ``PytestWarning`` is replaced with ``UserWarning``.
- Imports of unused pytest helpers (``assert_never``, ``skip``) are dropped.
- ``_ignore_error``, ``_IGNORED_ERRORS``, ``_IGNORED_WINERRORS`` are dropped:
  they are copied from CPython's ``pathlib`` for pytest's symlink-scan code
  path, which is not included here.

See the pytest license block in this repository's LICENSE file for the
applicable copyright notice.
"""

from __future__ import annotations

import atexit
import contextlib
import itertools
import os
import shutil
import sys
import types
import uuid
import warnings
from collections.abc import Callable, Iterable, Iterator
from functools import partial
from pathlib import Path, PurePath
from typing import TypeVar

LOCK_TIMEOUT = 60 * 60 * 24 * 3

_AnyPurePath = TypeVar("_AnyPurePath", bound=PurePath)


def get_lock_path(path: _AnyPurePath) -> _AnyPurePath:
    return path.joinpath(".lock")


def on_rm_rf_error(
    func: Callable[[str], object] | None,
    path: str,
    excinfo: BaseException
    | tuple[type[BaseException], BaseException, types.TracebackType | None],
    *,
    start_path: Path,
) -> bool:
    """Handle known read-only errors during rmtree.

    The returned value is used only by our own tests.
    """
    exc = excinfo if isinstance(excinfo, BaseException) else excinfo[1]

    # Another process removed the file in the middle of the "rm_rf" (xdist for example).
    # More context: https://github.com/pytest-dev/pytest/issues/5974#issuecomment-543799018
    if isinstance(exc, FileNotFoundError):
        return False

    if not isinstance(exc, PermissionError):
        warnings.warn(
            UserWarning(f"(rm_rf) error removing {path}\n{type(exc)}: {exc}"),
            stacklevel=2,
        )
        return False

    if func not in (os.rmdir, os.remove, os.unlink):
        if func not in (os.open,):
            warnings.warn(
                UserWarning(
                    f"(rm_rf) unknown function {func} when removing {path}:\n{type(exc)}: {exc}"
                ),
                stacklevel=2,
            )
        return False

    import stat

    def chmod_rw(p: str) -> None:
        mode = os.stat(p).st_mode
        os.chmod(p, mode | stat.S_IRUSR | stat.S_IWUSR)

    # For files, we need to recursively go upwards in the directories to
    # ensure they all are also writable.
    p = Path(path)
    if p.is_file():
        for parent in p.parents:
            chmod_rw(str(parent))
            if parent == start_path:
                break
    chmod_rw(str(path))

    func(path)
    return True


def ensure_extended_length_path(path: Path) -> Path:
    """Get the extended-length version of a path (Windows).

    On Windows, by default, the maximum length of a path (MAX_PATH) is 260
    characters, and operations on paths longer than that fail. But it is possible
    to overcome this by converting the path to "extended-length" form before
    performing the operation.

    On Windows, this function returns the extended-length absolute version of
    path. On other platforms it returns path unchanged.
    """
    if sys.platform.startswith("win32"):
        path = path.resolve()
        path = Path(get_extended_length_path_str(str(path)))
    return path


def get_extended_length_path_str(path: str) -> str:
    """Convert a path to a Windows extended length path."""
    long_path_prefix = "\\\\?\\"
    unc_long_path_prefix = "\\\\?\\UNC\\"
    if path.startswith((long_path_prefix, unc_long_path_prefix)):
        return path
    if path.startswith("\\\\"):
        return unc_long_path_prefix + path[2:]
    return long_path_prefix + path


def rm_rf(path: Path) -> None:
    """Remove the path contents recursively, even if some elements are read-only."""
    path = ensure_extended_length_path(path)
    onerror = partial(on_rm_rf_error, start_path=path)
    if sys.version_info >= (3, 12):
        shutil.rmtree(str(path), onexc=onerror)
    else:
        shutil.rmtree(str(path), onerror=onerror)


def find_prefixed(root: Path, prefix: str) -> Iterator[os.DirEntry[str]]:
    """Find all elements in root that begin with the prefix, case-insensitive."""
    l_prefix = prefix.lower()
    for x in os.scandir(root):
        if x.name.lower().startswith(l_prefix):
            yield x


def extract_suffixes(iter: Iterable[os.DirEntry[str]], prefix: str) -> Iterator[str]:
    """Return the parts of the paths following the prefix."""
    p_len = len(prefix)
    for entry in iter:
        yield entry.name[p_len:]


def find_suffixes(root: Path, prefix: str) -> Iterator[str]:
    """Combine find_prefixes and extract_suffixes."""
    return extract_suffixes(find_prefixed(root, prefix), prefix)


def parse_num(maybe_num: str) -> int:
    """Parse number path suffixes, returns -1 on error."""
    try:
        return int(maybe_num)
    except ValueError:
        return -1


def _force_symlink(root: Path, target: str | PurePath, link_to: str | Path) -> None:
    """Create the current symlink.

    It's full of race conditions that are reasonably OK to ignore for the
    context of best effort linking to the latest test run.
    """
    current_symlink = root.joinpath(target)
    with contextlib.suppress(OSError):
        current_symlink.unlink()
    with contextlib.suppress(OSError):
        current_symlink.symlink_to(link_to)


def make_numbered_dir(root: Path, prefix: str, mode: int = 0o700) -> Path:
    """Create a directory with an increased number as suffix for the given prefix."""
    last_error: OSError | None = None
    for _ in range(10):
        max_existing = max(map(parse_num, find_suffixes(root, prefix)), default=-1)
        new_number = max_existing + 1
        new_path = root.joinpath(f"{prefix}{new_number}")
        try:
            new_path.mkdir(mode=mode)
        except OSError as error:
            last_error = error
        else:
            _force_symlink(root, prefix + "current", new_path)
            return new_path
    message = (
        f"could not create numbered dir with prefix {prefix} in {root} after 10 tries"
    )
    raise OSError(message) from last_error


def create_cleanup_lock(p: Path) -> Path:
    """Create a lock to prevent premature directory cleanup."""
    lock_path = get_lock_path(p)
    try:
        fd = os.open(str(lock_path), os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o644)
    except FileExistsError as e:
        raise OSError(f"cannot create lockfile in {p}") from e
    pid = os.getpid()
    spid = str(pid).encode()
    os.write(fd, spid)
    os.close(fd)
    if not lock_path.is_file():
        raise OSError("lock path got renamed after successful creation")
    return lock_path


def register_cleanup_lock_removal(lock_path: Path) -> None:
    """Register a cleanup function for removing a lock, by default on atexit."""
    pid = os.getpid()

    def cleanup_on_exit(lock_path: Path = lock_path, original_pid: int = pid) -> None:
        current_pid = os.getpid()
        if current_pid != original_pid:
            return
        with contextlib.suppress(OSError):
            lock_path.unlink()

    atexit.register(cleanup_on_exit)


def maybe_delete_a_numbered_dir(path: Path) -> None:
    """Remove an unused numbered directory when its lock can be obtained."""
    path = ensure_extended_length_path(path)
    lock_path = None
    try:
        lock_path = create_cleanup_lock(path)
        parent = path.parent

        garbage = parent.joinpath(f"garbage-{uuid.uuid4()}")
        path.rename(garbage)
        rm_rf(garbage)
    except OSError:
        return
    finally:
        if lock_path is not None:
            with contextlib.suppress(OSError):
                lock_path.unlink()


def ensure_deletable(path: Path, consider_lock_dead_if_created_before: float) -> bool:
    """Check if `path` is deletable based on whether the lock file is expired."""
    if path.is_symlink():
        return False
    lock = get_lock_path(path)
    try:
        if not lock.is_file():
            return True
    except OSError:
        return False
    try:
        lock_time = lock.stat().st_mtime
    except OSError:
        return False
    else:
        if lock_time < consider_lock_dead_if_created_before:
            with contextlib.suppress(OSError):
                lock.unlink()
                return True
        return False


def try_cleanup(path: Path, consider_lock_dead_if_created_before: float) -> None:
    """Try to cleanup a directory if we can ensure it's deletable."""
    if ensure_deletable(path, consider_lock_dead_if_created_before):
        maybe_delete_a_numbered_dir(path)


def cleanup_candidates(root: Path, prefix: str, keep: int) -> Iterator[Path]:
    """List candidates for numbered directories to be removed - follows py.path."""
    max_existing = max(map(parse_num, find_suffixes(root, prefix)), default=-1)
    max_delete = max_existing - keep
    entries = find_prefixed(root, prefix)
    entries, entries2 = itertools.tee(entries)
    numbers = map(parse_num, extract_suffixes(entries2, prefix))
    for entry, number in zip(entries, numbers, strict=True):
        if number <= max_delete:
            yield Path(entry)


def cleanup_dead_symlinks(root: Path) -> None:
    """Remove broken symlinks from a directory."""
    for left_dir in root.iterdir():
        if left_dir.is_symlink() and not left_dir.resolve().exists():
            left_dir.unlink()


def cleanup_numbered_dir(
    root: Path, prefix: str, keep: int, consider_lock_dead_if_created_before: float
) -> None:
    """Cleanup for lock driven numbered directories."""
    if not root.exists():
        return
    for path in cleanup_candidates(root, prefix, keep):
        try_cleanup(path, consider_lock_dead_if_created_before)
    for path in root.glob("garbage-*"):
        try_cleanup(path, consider_lock_dead_if_created_before)

    cleanup_dead_symlinks(root)


def make_numbered_dir_with_cleanup(
    root: Path,
    prefix: str,
    keep: int,
    lock_timeout: float,
    mode: int,
) -> Path:
    """Create a numbered dir with a cleanup lock and remove old ones."""
    e: Exception | None = None
    for _ in range(10):
        try:
            p = make_numbered_dir(root, prefix, mode)
            if keep != 0:
                lock_path = create_cleanup_lock(p)
                register_cleanup_lock_removal(lock_path)
        except OSError as exc:
            e = exc
        else:
            consider_lock_dead_if_created_before = p.stat().st_mtime - lock_timeout
            atexit.register(
                cleanup_numbered_dir,
                root,
                prefix,
                keep,
                consider_lock_dead_if_created_before,
            )
            return p
    assert e is not None
    raise e
