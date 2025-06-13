from __future__ import annotations

import shutil
import subprocess
import tempfile
import textwrap
from dataclasses import dataclass
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from collections.abc import Iterable


class TestEnv:
    def __init__(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.project_dir = Path(self.temp_dir.name).resolve()
        self.remove_files()

    def remove_files(self) -> None:
        """Remove all files from the test environment."""
        for item in self.project_dir.iterdir():
            if item.is_file():
                item.unlink()
            elif item.is_dir():
                shutil.rmtree(item)

    def cleanup(self) -> None:
        """Clean up the test environment."""
        self.remove_files()
        self.temp_dir.cleanup()

    def write_files(self, files: Iterable[tuple[str, str]]) -> None:
        """Write multiple files to the test environment."""
        for path, content in files:
            self.write_file(path, content)

    def write_file(self, path: str | Path, content: str) -> None:
        """Write a single file to the test environment."""
        full_path = self.project_dir / path
        full_path.parent.mkdir(parents=True, exist_ok=True)

        content = textwrap.dedent(content)

        full_path.write_text(content)

    def run_test(self) -> CommandSnapshot:
        """Test the project and return (exit_code, stdout, stderr)."""
        karva_path = shutil.which("karva")
        if karva_path is None:
            msg = "Could not find karva executable in PATH"
            raise FileNotFoundError(msg)
        result = subprocess.run(  # noqa: S603
            [karva_path, "test", str(self.project_dir)],
            cwd=self.project_dir,
            check=False,
            capture_output=True,
            text=True,
        )
        output = CommandSnapshot(
            exit_code=result.returncode,
            stdout=result.stdout,
            stderr=result.stderr,
        )
        if result.returncode != 0:
            print(output.format())

        return output


@dataclass(eq=False)
class CommandSnapshot:
    exit_code: int
    stdout: str
    stderr: str

    def format(self) -> str:
        newline = "\n"
        return f"""success: {str(self.exit_code == 0).lower()}
exit_code: {self.exit_code}
----- stdout -----
{self.stdout}
----- stderr -----{f"{newline}{self.stderr}" if self.stderr else ""}"""

    def __eq__(self, other: object) -> bool:
        if isinstance(other, CommandSnapshot):
            return self.format() == other.format()
        if isinstance(other, str):
            return self.format() == textwrap.dedent(other)
        return False
