"""Check binary-size report deltas and safe rendering of function names."""

import json
import tempfile
from pathlib import Path

from binary_size import change, render


def test_report() -> None:
    """Cover growth, shrinkage, unchanged sizes, and escaped Rust symbols."""
    assert change(100, 110) == "+10 B (+10.00%)"
    assert change(100, 90) == "-10 B (-10.00%)"
    assert change(100, 100) == "+0 B (+0.00%)"
    assert change(0, 10) == "+10 B (n/a)"
    with tempfile.TemporaryDirectory() as temporary:
        directory = Path(temporary)
        for binary in ("karva", "karva-worker"):
            for revision, count in (("base", 100), ("head", 110)):
                (directory / f"{revision}-{binary}").write_bytes(b"x" * count)
                (directory / f"{revision}-{binary}.json").write_text(
                    json.dumps(
                        {
                            "text-section-size": count,
                            "functions": [
                                {"size": count, "name": "<Type as Trait>::call"}
                            ],
                        }
                    )
                )
        report = render(directory, "base-sha", "head-sha")
    assert report.count("+10 B (+10.00%)") == 4
    assert "`karva-worker` | Stripped file" in report
    assert "`karva` | `.text`" in report
    assert "&lt;Type as Trait&gt;::call" in report
    assert "[Unknown]" in report
    assert "Base `base-sha` → head `head-sha`" in report
    assert report.count("<details>") == report.count("</details>") == 2


if __name__ == "__main__":
    test_report()
