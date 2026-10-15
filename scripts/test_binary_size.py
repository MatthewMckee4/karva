"""Check the concise binary size summary and its collapsed details."""

import json
import tempfile
import unittest
from pathlib import Path

from binary_size import render


class BinarySizeTests(unittest.TestCase):
    """Keep status based on stripped files and all measurements in one dropdown."""

    def test_report(self):
        """Cover unchanged, increased, decreased, and opposing size changes."""
        for karva_delta, worker_delta in ((0, 0), (1, 1), (-1, -1), (1, -1)):
            with self.subTest(karva_delta=karva_delta, worker_delta=worker_delta):
                with tempfile.TemporaryDirectory() as temporary:
                    directory = Path(temporary)
                    for binary, delta in (
                        ("karva", karva_delta),
                        ("karva-worker", worker_delta),
                    ):
                        for revision, length in (("base", 10), ("head", 10 + delta)):
                            (directory / f"{revision}-{binary}").write_bytes(
                                b"x" * length
                            )
                            (directory / f"{revision}-{binary}.json").write_text(
                                json.dumps(
                                    {
                                        "text-section-size": 5
                                        if revision == "base"
                                        else 6,
                                        "functions": [
                                            {
                                                "size": 3,
                                                "crate": "karva",
                                                "name": "a<b>",
                                            }
                                        ],
                                    }
                                )
                            )
                    report = render(directory, "base-sha", "head-sha")
                status = {0: "unchanged", 1: "increased", -1: "decreased"}
                summary, details = report.split("<details>\n")
                assert summary == (
                    "<!-- karva-binary-size -->\n"
                    f"Binary size: `karva` {status[karva_delta]}; "
                    f"`karva-worker` {status[worker_delta]}.\n\n"
                )
                assert details.startswith("<summary>Details</summary>\n")
                assert details.endswith("</details>\n")
                assert report.count("<summary>") == 1
                assert "Base `base-sha` → head `head-sha`." in details
                assert "Linux x86-64 release builds" in details
                assert details.count("| Stripped file |") == 2
                assert details.count("| `.text` |") == 2
                assert "### karva: largest functions" in details
                assert "### karva-worker: largest functions" in details
                assert details.count("a&lt;b&gt;") == 4


if __name__ == "__main__":
    unittest.main()
