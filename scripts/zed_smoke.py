"""Prepare a clean Zed extension package and verify its configured server over stdio.

The fixture stays on disk for Install Dev Extension and Open Folder in real Zed.
This verifies the native protocol separately from the manual editor checklist.
"""

import argparse
import io
import json
import shutil
import subprocess
import tarfile
import tempfile
import tomllib
from pathlib import Path

from lsp_client import LspClient


def prepare(output, server, server_args, wasm, revision):
    """Use tracked committed extension sources, never an existing target or dev install."""
    output.mkdir(parents=True, exist_ok=False)
    archive = subprocess.check_output(
        ["git", "archive", revision, "editors/zed", "LICENSE", "rust-toolchain.toml"]
    )
    with tarfile.open(fileobj=io.BytesIO(archive)) as source:
        source.extractall(output, filter="data")
    extension = output / "editors/zed"
    shutil.copyfile(wasm, extension / "extension.wasm")
    manifest = tomllib.loads((extension / "extension.toml").read_text())
    assert manifest["language_servers"]["karva"]["languages"] == ["Python"]
    assert (extension / "extension.wasm").stat().st_size > 0
    assert (extension / "LICENSE").is_file()
    assert (output / "rust-toolchain.toml").is_file()
    for snippet in manifest.get("snippets", []):
        assert json.loads((extension / snippet).read_text())
    native = output / server.name
    shutil.copy2(server, native)
    workspace = output / "workspace"
    (workspace / ".zed").mkdir(parents=True)
    (workspace / "karva.toml").write_text(
        '[profile.default.test]\ntest-function-prefix = "test"\n'
    )
    (workspace / "test_smoke.py").write_text(
        "from karva import fixture\n\n@fixture\ndef sample():\n    return True\n\ndef test_smoke(sample):\n    assert sample\n"
    )
    (workspace / ".zed/settings.json").write_text(
        json.dumps(
            {
                "languages": {
                    "Python": {"language_servers": ["karva"], "format_on_save": "off"}
                },
                "lsp": {
                    "karva": {"binary": {"path": str(native), "arguments": server_args}}
                },
            },
            indent=2,
        )
        + "\n"
    )
    (workspace / ".zed/tasks.json").write_text(
        json.dumps(
            [
                {
                    "label": "Karva $ZED_CUSTOM_PYTHON_TEST_TARGET",
                    "command": "uv",
                    "args": ["run", "karva", "test", "$ZED_CUSTOM_PYTHON_TEST_TARGET"],
                    "cwd": "$ZED_WORKTREE_ROOT",
                    "save": "all",
                    "tags": ["python-pytest-method"],
                }
            ],
            indent=2,
        )
        + "\n"
    )
    return extension, workspace, [str(native), *server_args]


def smoke(command, workspace):
    """Check activation capabilities, a test runnable and configuration invalidation."""
    with tempfile.TemporaryFile(mode="w+b") as stderr:
        client = LspClient(command, workspace, stderr)
        _smoke(client, workspace)


def _smoke(client, workspace):
    try:
        result = client.request(
            "initialize",
            {
                "processId": None,
                "workspaceFolders": [{"uri": workspace.as_uri(), "name": "smoke"}],
                "capabilities": {
                    "workspace": {
                        "didChangeWatchedFiles": {"dynamicRegistration": True}
                    }
                },
            },
        )
        assert result["serverInfo"]["name"] == "karva"
        assert result["capabilities"]["codeLensProvider"]
        client.notify("initialized", {})
        path = workspace / "test_smoke.py"
        document = {"uri": path.as_uri()}
        client.notify(
            "textDocument/didOpen",
            {
                "textDocument": {
                    **document,
                    "languageId": "python",
                    "version": 1,
                    "text": path.read_text(),
                }
            },
        )
        lenses = client.request("textDocument/codeLens", {"textDocument": document})
        assert any("test_smoke" in json.dumps(lens) for lens in lenses), lenses
        config = workspace / "karva.toml"
        config.write_text('[profile.default.test]\ntest-function-prefix = "check"\n')
        client.notify(
            "workspace/didChangeWatchedFiles",
            {"changes": [{"uri": config.as_uri(), "type": 2}]},
        )
        assert client.request("textDocument/codeLens", {"textDocument": document}) == []
        config.write_text('[profile.default.test]\ntest-function-prefix = "test"\n')
        client.notify(
            "workspace/didChangeWatchedFiles",
            {"changes": [{"uri": config.as_uri(), "type": 2}]},
        )
        assert client.request("textDocument/codeLens", {"textDocument": document})
        client.request("shutdown", None)
        client.notify("exit", None)
        client.process.stdin.close()
        assert client.process.wait() == 0
    finally:
        client.close()


def main():
    """Prepare a disposable, reviewable fixture and run its protocol checks."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server", type=Path, required=True)
    parser.add_argument("--server-arg", action="append", default=[])
    parser.add_argument("--wasm", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--revision", default="HEAD")
    args = parser.parse_args()
    extension, workspace, command = prepare(
        args.output.resolve(),
        args.server.resolve(),
        args.server_arg,
        args.wasm.resolve(),
        args.revision,
    )
    smoke(command, workspace)
    print(
        f"Protocol smoke passed. Install Dev Extension: {extension}\nOpen Folder: {workspace}"
    )


if __name__ == "__main__":
    main()
