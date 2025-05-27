"""Karva is a Python test runner, written in Rust."""

from __future__ import annotations

import os
import sys
import sysconfig
from pathlib import Path

MAX_PATH_PARTS = 3


def find_karva_bin() -> str:
    """Return the karva binary path."""
    karva_exe = "karva" + sysconfig.get_config_var("EXE")

    scripts_path = Path(sysconfig.get_path("scripts")) / karva_exe
    if scripts_path.is_file():
        return str(scripts_path)

    if sys.version_info >= (3, 10):
        user_scheme = sysconfig.get_preferred_scheme("user")
    elif os.name == "nt":
        user_scheme = "nt_user"
    elif sys.platform == "darwin" and sys._framework:
        user_scheme = "osx_framework_user"
    else:
        user_scheme = "posix_user"

    user_path = Path(sysconfig.get_path("scripts", scheme=user_scheme)) / karva_exe
    if user_path.is_file():
        return str(user_path)

    # Search in `bin` adjacent to package root (as created by `pip install --target`).
    pkg_root = Path(__file__).parent.parent
    target_path = pkg_root / "bin" / karva_exe
    if target_path.is_file():
        return str(target_path)

    paths = os.environ.get("PATH", "").split(os.pathsep)
    if len(paths) >= 2:

        def get_last_three_path_parts(path: str) -> list[str]:
            """Return a list of up to the last three parts of a path."""
            parts: list[str] = []

            while len(parts) < MAX_PATH_PARTS:
                head, tail = os.path.split(path)
                if tail or head != path:
                    parts.append(tail)
                    path = head
                else:
                    parts.append(path)
                    break

            return parts

        maybe_overlay = get_last_three_path_parts(paths[0])
        maybe_normal = get_last_three_path_parts(paths[1])
        if (
            len(maybe_normal) >= MAX_PATH_PARTS
            and maybe_normal[-1].startswith("pip-build-env-")
            and maybe_normal[-2] == "normal"
            and len(maybe_overlay) >= MAX_PATH_PARTS
            and maybe_overlay[-1].startswith("pip-build-env-")
            and maybe_overlay[-2] == "overlay"
        ):
            # The overlay must contain the karva binary.
            candidate = Path(paths[0]) / karva_exe
            if candidate.is_file():
                return str(candidate)

    raise FileNotFoundError(scripts_path)


if __name__ == "__main__":
    karva = os.fsdecode(find_karva_bin())
    if sys.platform == "win32":
        import subprocess

        completed_process = subprocess.run([karva, *sys.argv[1:]], check=True)  # noqa: S603
        sys.exit(completed_process.returncode)
    else:
        os.execvp(karva, [karva, *sys.argv[1:]])  # noqa: S606
