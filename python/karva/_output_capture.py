"""Reuse a worker's native capture streams while restoring descriptors per attempt."""

import ctypes
import io
import os
import sys
from contextlib import ExitStack

_libc = ctypes.CDLL("ucrtbase" if os.name == "nt" else None)
_libc.fflush.argtypes = [ctypes.c_void_p]
_libc.fflush.restype = ctypes.c_int


class _Capture:
    """Worker-lifetime streams for non-nested attempts; closing restores current state."""

    def __init__(self) -> None:
        with ExitStack() as rollback:
            stdout = os.dup(1)
            rollback.callback(os.close, stdout)
            stderr = os.dup(2)
            rollback.callback(os.close, stderr)
            self.saved = (stdout, stderr)
            self.inheritable = (os.get_inheritable(1), os.get_inheritable(2))
            self.stdout = self._stream(1)
            self.stderr = self._stream(2)
            self.original = (sys.stdout, sys.stderr)
            rollback.pop_all()

    @staticmethod
    def _stream(descriptor: int) -> io.TextIOWrapper:
        return io.TextIOWrapper(
            os.fdopen(descriptor, "wb", buffering=0, closefd=False),
            encoding="utf-8",
            errors="backslashreplace",
            newline="",
            write_through=True,
        )

    def start(self) -> None:
        self.original = (sys.stdout, sys.stderr)
        for stream in self.original:
            stream.flush()
        if self.stdout.closed:
            self.stdout = self._stream(1)
        if self.stderr.closed:
            self.stderr = self._stream(2)
        sys.stdout, sys.stderr = self.stdout, self.stderr

    def close(self) -> None:
        """Restore both descriptors even when test code closes or redirects them."""
        _libc.fflush(None)
        sys.stdout, sys.stderr = self.original
        try:
            os.dup2(self.saved[0], 1, inheritable=self.inheritable[0])
        finally:
            os.dup2(self.saved[1], 2, inheritable=self.inheritable[1])


def create_capture() -> _Capture:
    """Create one worker-lifetime route; Rust caches its start and close methods."""
    return _Capture()
