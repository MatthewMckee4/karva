"""Redirect worker descriptors to native capture pipes with rollback on failure."""

import ctypes
import io
import os
import sys
from contextlib import ExitStack

# Python uses the universal CRT on Windows; Unix C extensions share libc.
_libc = ctypes.CDLL("ucrtbase" if os.name == "nt" else None)
_libc.fflush.argtypes = [ctypes.c_void_p]
_libc.fflush.restype = ctypes.c_int


def _pipe_fd(native: int) -> int:
    if os.name != "nt":
        return os.dup(native)

    import _winapi
    import msvcrt

    process = _winapi.GetCurrentProcess()
    handle = _winapi.DuplicateHandle(
        process, native, process, 0, False, _winapi.DUPLICATE_SAME_ACCESS
    )
    try:
        return msvcrt.open_osfhandle(handle, os.O_WRONLY | os.O_BINARY)
    except BaseException:
        _winapi.CloseHandle(handle)
        raise


def redirect_output(stdout: int, stderr: int) -> ExitStack:
    """Restore Python streams and descriptors when the returned stack closes.

    Unbuffered binary streams keep Python/native write order and make output
    available to the native deadline watchdog without acquiring Python's GIL.
    """
    stack = ExitStack()
    try:
        for name, descriptor, native in [("stdout", 1, stdout), ("stderr", 2, stderr)]:
            original = getattr(sys, name)
            original.flush()
            saved = os.dup(descriptor)
            stack.callback(os.close, saved)
            stack.callback(
                os.dup2, saved, descriptor, inheritable=os.get_inheritable(descriptor)
            )
            pipe = _pipe_fd(native)
            try:
                os.dup2(pipe, descriptor)
            finally:
                os.close(pipe)
            stream = io.TextIOWrapper(
                os.fdopen(os.dup(descriptor), "wb", buffering=0),
                encoding="utf-8",
                errors="backslashreplace",
                newline="",
                write_through=True,
            )
            stack.callback(stream.close)
            stack.callback(setattr, sys, name, original)
            setattr(sys, name, stream)
        stack.callback(_libc.fflush, None)
    except BaseException:
        stack.close()
        raise
    return stack
