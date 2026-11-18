"""Compute cache identity without exposing environment values in result data."""

import hashlib
import importlib.metadata
import io
import json
import os
import sys
from pathlib import Path


def fingerprint():
    """Hash runtime, environment, installed files, and editable Python source."""
    digest = hashlib.sha256()

    def add(value):
        digest.update(str(len(value)).encode())
        digest.update(b":")
        digest.update(value)

    add(sys.version.encode())
    add(sys.executable.encode())
    add(json.dumps(sys.path).encode())
    environment = {
        key: value
        for key, value in os.environ.items()
        if key not in {"PWD", "OLDPWD", "SHLVL", "_"}
    }
    add(json.dumps(environment, sort_keys=True).encode())
    paths = set()
    distributions = sorted(
        importlib.metadata.distributions(),
        key=lambda item: (
            item.metadata.get("Name", ""),
            item.version,
            str(item.locate_file("")),
        ),
    )
    for distribution in distributions:
        add(distribution.metadata.get("Name", "").encode())
        add(distribution.version.encode())
        for entry in distribution.files or ():
            path = Path(distribution.locate_file(entry))
            if path.suffix != ".pyc" and path.is_file():
                paths.add(path.resolve())
        direct = distribution.read_text("direct_url.json")
        if direct:
            add(direct.encode())
            data = json.loads(direct)
            if data.get("dir_info", {}).get("editable"):
                from urllib.parse import urlparse
                from urllib.request import url2pathname

                root = Path(url2pathname(urlparse(data["url"]).path))
                for directory, children, filenames in os.walk(root):
                    children[:] = [
                        name
                        for name in children
                        if not name.startswith(".")
                        and name not in {"__pycache__", "venv", "target"}
                    ]
                    paths.update(
                        Path(directory) / name
                        for name in filenames
                        if name.endswith(".py")
                    )
    for path in sorted(paths):
        add(str(path).encode())
        content = hashlib.sha256()
        with path.open("rb") as stream:
            while chunk := stream.read(io.DEFAULT_BUFFER_SIZE):
                content.update(chunk)
        add(content.digest())
    return digest.hexdigest()
