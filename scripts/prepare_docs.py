# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///

from pathlib import Path

ROOT = Path(__file__).parent.parent


def prepare_index_file() -> None:
    """Copy root `README.md` to `docs/index.md`"""
    (ROOT / "docs" / "index.md").write_text((ROOT / "README.md").read_text())


def main() -> None:
    prepare_index_file()


if __name__ == "__main__":
    main()
