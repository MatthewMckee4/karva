"""Render stripped binary and cargo-bloat code sizes for the PR comment."""

import html
import json
import os
import sys
from pathlib import Path


def size(value: int) -> str:
    """Show readable sizes alongside exact bytes so small changes stay visible."""
    return f"{value / 1024**2:.2f} MiB ({value:,} B)"


def change(base: int, head: int) -> str:
    """Handle newly introduced code sections without dividing by zero."""
    percent = f"{(head - base) / base:+.2%}" if base else "n/a"
    return f"{head - base:+,} B ({percent})"


def render(directory: Path, base_sha: str, head_sha: str) -> str:
    """Read measurements for both revisions and include the largest functions."""
    lines = [
        "<details>",
        "<summary>Details</summary>",
        "",
        f"Base `{base_sha}` → head `{head_sha}`.",
        "",
        (
            "Linux x86-64 release builds with the same Rust toolchain and Python 3.13. "
            "File size is measured after stripping symbols; `.text` is executable code "
            "measured by cargo-bloat before stripping. Shared libraries (including Python) "
            "are excluded. Function attribution is approximate."
        ),
        "",
        "| Binary | Measurement | Base | Head | Change |",
        "| --- | --- | --- | --- | --- |",
    ]
    statuses = []
    details = []
    for binary in ("karva", "karva-worker"):
        base_size = (directory / f"base-{binary}").stat().st_size
        head_size = (directory / f"head-{binary}").stat().st_size
        if head_size > base_size:
            status = "increased"
        elif head_size < base_size:
            status = "decreased"
        else:
            status = "unchanged"
        statuses.append(f"`{binary}` {status}")
        measurements = {
            revision: json.loads((directory / f"{revision}-{binary}.json").read_text())
            for revision in ("base", "head")
        }
        for label, base, head in (
            (
                "Stripped file",
                base_size,
                head_size,
            ),
            (
                "`.text`",
                measurements["base"]["text-section-size"],
                measurements["head"]["text-section-size"],
            ),
        ):
            lines.append(
                f"| `{binary}` | {label} | {size(base)} | {size(head)} | {change(base, head)} |"
            )
        details.extend(["", f"### {binary}: largest functions", ""])
        for revision in ("head", "base"):
            details.extend([f"#### {revision.title()} Branch Results", "", "<pre>"])
            details.append("     Bytes  Crate  Function")
            for function in measurements[revision]["functions"]:
                details.append(
                    html.escape(
                        f"{function['size']:>10,}  {function.get('crate', '[Unknown]')}  "
                        f"{function['name']}"
                    )
                )
            details.extend(["</pre>", ""])
    return "\n".join(
        [
            "<!-- karva-binary-size -->",
            f"Binary size: {'; '.join(statuses)}.",
            "",
            *lines,
            *details,
            "</details>",
            "",
        ]
    )


def main() -> None:
    """Write human-readable output and metadata for the isolated comment job."""
    directory = Path(sys.argv[1])
    base_sha = os.environ["BASE_SHA"]
    head_sha = os.environ["HEAD_SHA"]
    (directory / "report.md").write_text(render(directory, base_sha, head_sha))
    (directory / "report.json").write_text(
        json.dumps(
            {
                "pr_number": int(os.environ["PR_NUMBER"]),
                "base_sha": base_sha,
                "head_sha": head_sha,
            }
        )
    )


if __name__ == "__main__":
    main()
