# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///

"""Verify the release server's protocol without a Python runtime or Karva wheel."""

import argparse
import tempfile
from pathlib import Path

from zed_smoke import smoke


def main():
    """Exercise initialization, diagnostics, code lenses, reload and shutdown."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server", type=Path, required=True)
    args = parser.parse_args()
    server = args.server.resolve()
    if not server.is_file() and server.with_suffix(".exe").is_file():
        server = server.with_suffix(".exe")
    with tempfile.TemporaryDirectory(prefix="karva-standalone-") as directory:
        workspace = Path(directory).resolve()
        (workspace / "karva.toml").write_text(
            '[profile.default.test]\ntest-function-prefix = "test"\n'
        )
        (workspace / "test_smoke.py").write_text(
            "from karva import fixture\n\n@fixture\ndef sample():\n"
            "    return True\n\ndef test_smoke(sample):\n    assert sample\n"
        )
        smoke([str(server)], workspace)
    print("Standalone stdio protocol passed")


if __name__ == "__main__":
    main()
