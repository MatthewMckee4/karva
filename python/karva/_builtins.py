"""Built-in fixtures provided by the Karva test framework.

These fixtures are automatically available to all tests without any imports.

Several fixtures (``monkeypatch``, ``recwarn``, ``tmp_path`` and friends) are
thin wrappers around classes vendored from pytest — see
``karva._vendor._pytest_monkeypatch``, ``karva._vendor._pytest_recwarn``, and
``karva._vendor._pytest_tmpdir`` for the vendored implementations and the
LICENSE file at the repository root for the applicable copyright notices.

The fixture wrappers themselves have to live at the top level of this module
because the Rust-side framework fixture discoverer parses this file's AST
looking for ``@fixture``-decorated ``def`` statements.
"""

from __future__ import annotations

import io
import logging
import os
import sys
import tempfile
import warnings
from collections.abc import Generator
from typing import TYPE_CHECKING, BinaryIO, NamedTuple, TextIO, cast

if TYPE_CHECKING:
    from pathlib import Path
    from typing import Self

from karva._karva import fixture
from karva._vendor._pytest_monkeypatch import MockEnv
from karva._vendor._pytest_recwarn import WarningsRecorder
from karva._vendor._pytest_tmpdir import TempPathFactory

__all__ = [
    "CaptureResult",
    "MockEnv",
    "capfd",
    "capfdbinary",
    "caplog",
    "capsys",
    "capsysbinary",
    "monkeypatch",
    "recwarn",
    "temp_dir",
    "temp_path",
    "tmp_path",
    "tmp_path_factory",
    "tmpdir",
    "tmpdir_factory",
]


@fixture
def monkeypatch() -> Generator[MockEnv, None, None]:
    """Fixture that provides a :class:`MockEnv` for patching during a test."""
    mpatch = MockEnv()
    yield mpatch
    mpatch.undo()


class CaptureResult(NamedTuple):
    """Captured stdout and stderr from :fixture:`capsys` / :fixture:`capfd`."""

    out: str
    err: str


class _CapsysDisabled:
    """Context manager that temporarily restores real stdout/stderr during capture."""

    def __init__(
        self,
        real_stdout: TextIO,
        real_stderr: TextIO,
        cur_out: io.StringIO,
        cur_err: io.StringIO,
    ) -> None:
        self._real_stdout: TextIO = real_stdout
        self._real_stderr: TextIO = real_stderr
        self._cur_out: io.StringIO = cur_out
        self._cur_err: io.StringIO = cur_err

    def __enter__(self) -> Self:
        sys.stdout = self._real_stdout
        sys.stderr = self._real_stderr
        return self

    def __exit__(self, *args: object) -> bool:
        sys.stdout = self._cur_out
        sys.stderr = self._cur_err
        return False


class _CapsysFixture:
    """Captures writes to ``sys.stdout`` and ``sys.stderr`` as strings."""

    def __init__(self, real_stdout: TextIO, real_stderr: TextIO) -> None:
        self._real_stdout: TextIO = real_stdout
        self._real_stderr: TextIO = real_stderr
        self._out: io.StringIO = io.StringIO()
        self._err: io.StringIO = io.StringIO()
        sys.stdout = self._out
        sys.stderr = self._err

    def readouterr(self) -> CaptureResult:
        """Return captured output and reset the buffers."""
        out = self._out.getvalue()
        err = self._err.getvalue()
        self._out = io.StringIO()
        self._err = io.StringIO()
        sys.stdout = self._out
        sys.stderr = self._err
        return CaptureResult(out, err)

    def disabled(self) -> _CapsysDisabled:
        """Context manager that temporarily restores real stdout/stderr."""
        return _CapsysDisabled(
            self._real_stdout, self._real_stderr, self._out, self._err
        )

    def __repr__(self) -> str:
        return "<CapsysFixture object>"


class _CapfdDisabled:
    """Context manager that temporarily restores real file descriptors."""

    def __init__(self, fixture: _CapfdFixture) -> None:
        self._fixture = fixture

    def __enter__(self) -> Self:
        self._fixture._suspend()
        return self

    def __exit__(self, *args: object) -> bool:
        self._fixture._resume()
        return False


class _CapfdFixture:
    """Captures writes to stdout and stderr at the file-descriptor level."""

    def __init__(self, real_stdout: TextIO, real_stderr: TextIO) -> None:
        self._real_stdout = real_stdout
        self._real_stderr = real_stderr
        self._saved_stdout_fd = os.dup(1)
        self._saved_stderr_fd = os.dup(2)
        self._out_file: BinaryIO = tempfile.TemporaryFile(mode="w+b")  # noqa: SIM115
        self._err_file: BinaryIO = tempfile.TemporaryFile(mode="w+b")  # noqa: SIM115
        os.dup2(self._out_file.fileno(), 1)
        os.dup2(self._err_file.fileno(), 2)
        self._stdout = io.TextIOWrapper(
            os.fdopen(os.dup(1), "wb"),
            encoding=getattr(real_stdout, "encoding", None) or "utf-8",
            newline="",
            write_through=True,
        )
        self._stderr = io.TextIOWrapper(
            os.fdopen(os.dup(2), "wb"),
            encoding=getattr(real_stderr, "encoding", None) or "utf-8",
            newline="",
            write_through=True,
        )
        sys.stdout = self._stdout
        sys.stderr = self._stderr

    def readouterr(self) -> CaptureResult:
        """Return captured output and reset the temporary files."""
        self._flush()
        self._out_file.seek(0)
        out = self._out_file.read().decode(self._stdout.encoding)
        self._out_file.seek(0)
        self._out_file.truncate()
        self._err_file.seek(0)
        err = self._err_file.read().decode(self._stderr.encoding)
        self._err_file.seek(0)
        self._err_file.truncate()
        return CaptureResult(out, err)

    def disabled(self) -> _CapfdDisabled:
        """Context manager that temporarily restores real file descriptors."""
        return _CapfdDisabled(self)

    def close(self) -> None:
        """Restore stdout and stderr and close the capture files."""
        self._suspend()
        self._stdout.close()
        self._stderr.close()
        self._out_file.close()
        self._err_file.close()
        os.close(self._saved_stdout_fd)
        os.close(self._saved_stderr_fd)

    def _flush(self) -> None:
        self._stdout.flush()
        self._stderr.flush()

    def _suspend(self) -> None:
        self._flush()
        os.dup2(self._saved_stdout_fd, 1)
        os.dup2(self._saved_stderr_fd, 2)
        sys.stdout = self._real_stdout
        sys.stderr = self._real_stderr

    def _resume(self) -> None:
        os.dup2(self._out_file.fileno(), 1)
        os.dup2(self._err_file.fileno(), 2)
        sys.stdout = self._stdout
        sys.stderr = self._stderr

    def __repr__(self) -> str:
        return "<CapfdFixture object>"


class BinaryCaptureResult(NamedTuple):
    """Captured stdout and stderr from :fixture:`capsysbinary` / :fixture:`capfdbinary`."""

    out: bytes
    err: bytes


class _BinaryCaptureStream:
    """Accepts both str and bytes writes, stores raw bytes."""

    def __init__(self) -> None:
        self._data: bytearray = bytearray()

    def write(self, obj: str | bytes | bytearray) -> int:
        if isinstance(obj, str):
            b = obj.encode("utf-8")
        elif isinstance(obj, (bytes, bytearray)):
            b = bytes(obj)
        else:
            raise TypeError("write() argument must be str or bytes-like object")
        self._data += b
        return len(b)

    def flush(self) -> None:
        pass

    def getvalue(self) -> bytes:
        return bytes(self._data)

    @property
    def encoding(self) -> str:
        return "utf-8"


class _CapsysBinaryDisabled:
    """Context manager that temporarily restores real stdout/stderr during binary capture."""

    def __init__(
        self,
        real_stdout: TextIO,
        real_stderr: TextIO,
        cur_out: _BinaryCaptureStream,
        cur_err: _BinaryCaptureStream,
    ) -> None:
        self._real_stdout: TextIO = real_stdout
        self._real_stderr: TextIO = real_stderr
        self._cur_out: _BinaryCaptureStream = cur_out
        self._cur_err: _BinaryCaptureStream = cur_err

    def __enter__(self) -> Self:
        sys.stdout = self._real_stdout
        sys.stderr = self._real_stderr
        return self

    def __exit__(self, *args: object) -> bool:
        sys.stdout = cast(TextIO, self._cur_out)
        sys.stderr = cast(TextIO, self._cur_err)
        return False


class _CapsysBinaryFixture:
    """Captures writes to ``sys.stdout`` and ``sys.stderr`` as bytes."""

    def __init__(self, real_stdout: TextIO, real_stderr: TextIO) -> None:
        self._real_stdout: TextIO = real_stdout
        self._real_stderr: TextIO = real_stderr
        self._out: _BinaryCaptureStream = _BinaryCaptureStream()
        self._err: _BinaryCaptureStream = _BinaryCaptureStream()
        sys.stdout = cast(TextIO, self._out)
        sys.stderr = cast(TextIO, self._err)

    def readouterr(self) -> BinaryCaptureResult:
        """Return captured output as bytes and reset the buffers."""
        out = self._out.getvalue()
        err = self._err.getvalue()
        self._out = _BinaryCaptureStream()
        self._err = _BinaryCaptureStream()
        sys.stdout = cast(TextIO, self._out)
        sys.stderr = cast(TextIO, self._err)
        return BinaryCaptureResult(out, err)

    def disabled(self) -> _CapsysBinaryDisabled:
        """Context manager that temporarily restores real stdout/stderr."""
        return _CapsysBinaryDisabled(
            self._real_stdout, self._real_stderr, self._out, self._err
        )

    def __repr__(self) -> str:
        return "<CapsysBinaryFixture object>"


class _CapLogHandler(logging.Handler):
    """Logging handler that captures records into an owned list."""

    def __init__(self) -> None:
        super().__init__(0)
        self.records: list[logging.LogRecord] = []

    def emit(self, record: logging.LogRecord) -> None:
        record.message = record.getMessage()
        self.records.append(record)


class _CapLogAtLevel:
    """Context manager that temporarily sets the log capture level."""

    def __init__(
        self,
        handler: _CapLogHandler,
        level: int,
        logger_name: str | None,
    ) -> None:
        self._handler: _CapLogHandler = handler
        self._level: int = level
        self._logger_name: str | None = logger_name
        self._prev_handler_level: int = handler.level
        self._prev_logger_level: int | None = None

    def __enter__(self) -> Self:
        logger = logging.getLogger(self._logger_name)
        self._prev_logger_level = logger.level
        logger.setLevel(self._level)
        self._handler.setLevel(self._level)
        return self

    def __exit__(self, *args: object) -> bool:
        logger = logging.getLogger(self._logger_name)
        if self._prev_logger_level is not None:
            logger.setLevel(self._prev_logger_level)
        self._handler.setLevel(self._prev_handler_level)
        return False


class _CapLog:
    """Exposes captured log records to the test."""

    def __init__(self, handler: _CapLogHandler) -> None:
        self._handler: _CapLogHandler = handler
        # Records the original level for every logger that ``set_level`` has
        # touched, keyed by logger name (``None`` for the root logger). Stored
        # so teardown can restore every touched logger, not just the first.
        self._saved_levels: dict[str | None, int] = {}
        self._saved_handler_level: int | None = None

    @property
    def records(self) -> list[logging.LogRecord]:
        return self._handler.records

    @property
    def handler(self) -> _CapLogHandler:
        return self._handler

    @property
    def record_tuples(self) -> list[tuple[str, int, str]]:
        return [(r.name, r.levelno, r.getMessage()) for r in self._handler.records]

    @property
    def messages(self) -> list[str]:
        return [r.getMessage() for r in self._handler.records]

    @property
    def text(self) -> str:
        formatter = logging.Formatter()
        return "\n".join(formatter.format(r) for r in self._handler.records)

    def set_level(self, level: int, logger: str | None = None) -> None:
        """Set the capture level for the remainder of the test.

        Remembers the original level of every logger touched so the caplog
        teardown can restore all of them.
        """
        target = logging.getLogger(logger)
        if logger not in self._saved_levels:
            self._saved_levels[logger] = target.level
        if self._saved_handler_level is None:
            self._saved_handler_level = self._handler.level
        target.setLevel(level)
        self._handler.setLevel(level)

    def _restore_levels(self) -> None:
        """Restore every logger that was touched via ``set_level``."""
        for logger_name, original_level in self._saved_levels.items():
            logging.getLogger(logger_name).setLevel(original_level)
        self._saved_levels.clear()
        if self._saved_handler_level is not None:
            self._handler.setLevel(self._saved_handler_level)
            self._saved_handler_level = None

    def at_level(self, level: int, logger: str | None = None) -> _CapLogAtLevel:
        """Context manager that temporarily sets the capture level."""
        return _CapLogAtLevel(self._handler, level, logger)

    def clear(self) -> None:
        """Clear all captured records."""
        self._handler.records.clear()

    def __repr__(self) -> str:
        return "<CapLog object>"


def _capsys_impl() -> Generator[_CapsysFixture, None, None]:
    """Shared generator for capsys and capfd."""
    real_stdout = sys.stdout
    real_stderr = sys.stderr
    saved_disable: int = logging.root.manager.disable
    logging.disable(logging.NOTSET)
    f = _CapsysFixture(real_stdout, real_stderr)
    yield f
    sys.stdout = real_stdout
    sys.stderr = real_stderr
    logging.disable(saved_disable)


@fixture
def capsys() -> Generator[_CapsysFixture, None, None]:
    """Capture writes to ``sys.stdout`` and ``sys.stderr``."""
    yield from _capsys_impl()


@fixture
def capfd() -> Generator[_CapfdFixture, None, None]:
    """Capture writes to file descriptors 1 and 2 as strings."""
    saved_disable: int = logging.root.manager.disable
    logging.disable(logging.NOTSET)
    capture = _CapfdFixture(sys.stdout, sys.stderr)
    yield capture
    capture.close()
    logging.disable(saved_disable)


def _capsysbinary_impl() -> Generator[_CapsysBinaryFixture, None, None]:
    """Shared generator for capsysbinary and capfdbinary."""
    real_stdout = sys.stdout
    real_stderr = sys.stderr
    saved_disable: int = logging.root.manager.disable
    logging.disable(logging.NOTSET)
    f = _CapsysBinaryFixture(real_stdout, real_stderr)
    yield f
    sys.stdout = real_stdout
    sys.stderr = real_stderr
    logging.disable(saved_disable)


@fixture
def capsysbinary() -> Generator[_CapsysBinaryFixture, None, None]:
    """Capture writes to ``sys.stdout`` and ``sys.stderr`` as bytes."""
    yield from _capsysbinary_impl()


@fixture
def capfdbinary() -> Generator[_CapsysBinaryFixture, None, None]:
    """Capture writes to ``sys.stdout`` and ``sys.stderr`` as bytes (fd-level alias)."""
    yield from _capsysbinary_impl()


@fixture
def caplog() -> Generator[_CapLog, None, None]:
    """Capture log records emitted during a test."""
    saved_disable: int = logging.root.manager.disable
    handler = _CapLogHandler()
    cap = _CapLog(handler)
    root_logger = logging.getLogger()
    root_logger.addHandler(handler)
    logging.disable(logging.NOTSET)

    yield cap

    root_logger.removeHandler(handler)
    logging.disable(saved_disable)
    cap._restore_levels()


@fixture
def tmp_path(tmp_path_factory: TempPathFactory) -> Path:
    """Provide a temporary directory as a :class:`pathlib.Path` object."""
    return tmp_path_factory.mktemp("test")


@fixture
def temp_path(tmp_path_factory: TempPathFactory) -> Path:
    """Alias for :fixture:`tmp_path`."""
    return tmp_path_factory.mktemp("test")


@fixture
def temp_dir(tmp_path_factory: TempPathFactory) -> Path:
    """Alias for :fixture:`tmp_path`."""
    return tmp_path_factory.mktemp("test")


@fixture
def tmpdir(tmp_path_factory: TempPathFactory) -> Path:
    """Provide a temporary directory as a :class:`pathlib.Path`."""
    return tmp_path_factory.mktemp("test")


@fixture(scope="session")
def tmp_path_factory() -> TempPathFactory:
    """Session-scoped factory for creating numbered temporary directories."""
    return TempPathFactory()


@fixture(scope="session")
def tmpdir_factory() -> TempPathFactory:
    """Session-scoped factory for creating numbered temporary directories."""
    return TempPathFactory()


@fixture
def recwarn() -> Generator[WarningsRecorder, None, None]:
    """Return a :class:`WarningsRecorder` that records warnings raised during a test."""
    wrec = WarningsRecorder()
    with wrec:
        warnings.simplefilter("default")
        yield wrec
