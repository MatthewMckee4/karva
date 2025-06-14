from __future__ import annotations

import re
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
        self.temp_dir = tempfile.mkdtemp()
        self.project_dir = Path(self.temp_dir).resolve()
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
        shutil.rmtree(self.temp_dir)

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
        return CommandSnapshot(
            project_dir=self.project_dir,
            exit_code=result.returncode,
            stdout=result.stdout,
            stderr=result.stderr,
        )


@dataclass(eq=False)
class CommandSnapshot:
    project_dir: Path
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

    @classmethod
    def from_str(cls, s: str, project_dir: Path) -> CommandSnapshot:
        s = textwrap.dedent(s)
        lines = s.strip().split("\n")
        exit_code = int(lines[1].split(": ")[1])

        stdout_start = lines.index("----- stdout -----")
        stderr_start = lines.index("----- stderr -----")

        stdout = "\n".join(lines[stdout_start + 1 : stderr_start]).strip()
        stderr = "\n".join(lines[stderr_start + 1 :]).strip() if stderr_start + 1 < len(lines) else ""

        return cls(
            project_dir=project_dir,
            exit_code=exit_code,
            stdout=stdout,
            stderr=stderr,
        )

    def __eq__(self, other: object) -> bool:
        if isinstance(other, CommandSnapshot):

            def filter_line(line: str) -> str:
                line = line.replace("\\", "/")
                line = re.sub(r"\\(\w\w|\s|\.|\")", r"/\1", line)
                project_dir = str(self.project_dir).replace("\\", "/")
                return line.replace(project_dir, "<temp_dir>")

            def filter_lines(lines: list[str]) -> list[str]:
                return [filter_line(line) for line in lines]

            self_stdout_lines = set(filter_lines(self.stdout.splitlines()))
            other_stdout_lines = set(filter_lines(other.stdout.splitlines()))
            self_stderr_lines = set(filter_lines(self.stderr.splitlines()))
            other_stderr_lines = set(filter_lines(other.stderr.splitlines()))
            return (
                self.exit_code == other.exit_code
                and self_stdout_lines == other_stdout_lines
                and self_stderr_lines == other_stderr_lines
            )
        if isinstance(other, str):
            other_snapshot = CommandSnapshot.from_str(other, self.project_dir)
            res = self == other_snapshot
            if not res:
                print("Expected--------------------------------")
                print(other_snapshot.format())
                print("\nActual--------------------------------")
                print(self.format())
            return res
        return False
