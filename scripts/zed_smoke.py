# /// script
# requires-python = ">=3.13"
# dependencies = []
# ///

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


class LspClient:
    """One synchronous stdio peer; server stderr cannot fill a pipe."""

    def __init__(self, command, cwd, stderr):
        """Launch a peer with stderr owned by the caller's context manager."""
        self.stderr = stderr
        self.process = subprocess.Popen(
            command,
            cwd=cwd,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self.stderr,
        )
        self.next_id = 0
        self.notifications = []

    def send(self, message):
        """Write one complete LSP frame."""
        body = json.dumps({"jsonrpc": "2.0", **message}).encode()
        self.process.stdin.write(f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
        self.process.stdin.flush()

    def receive(self):
        """Read one complete frame, reporting early process failure."""
        headers = {}
        while (line := self.process.stdout.readline()) not in (b"\r\n", b"\n"):
            if not line:
                self.stderr.seek(0)
                raise RuntimeError(
                    f"Server closed stdout: {self.stderr.read().decode(errors='replace')}"
                )
            key, value = line.decode().split(":", 1)
            headers[key.lower()] = value.strip()
        return json.loads(self.process.stdout.read(int(headers["content-length"])))

    def request(self, method, params):
        """Wait for a matching response while accepting file-watcher registration."""
        self.next_id += 1
        request_id = self.next_id
        self.send({"id": request_id, "method": method, "params": params})
        while True:
            message = self.receive()
            if "method" in message and "id" in message:
                # Dynamic file watching registration is accepted by this fixture.
                if message["method"] != "client/registerCapability":
                    raise RuntimeError(f"Unexpected server request: {message}")
                self.send({"id": message["id"], "result": None})
            elif message.get("id") == request_id:
                if "error" in message:
                    raise RuntimeError(message["error"])
                return message.get("result")
            else:
                self.notifications.append(message)

    def notify(self, method, params):
        """Send a notification without expecting a response."""
        self.send({"method": method, "params": params})

    def close(self):
        """Reap the subprocess after either successful shutdown or failure."""
        self.process.kill()
        self.process.wait()
        self.process.stdin.close()
        self.process.stdout.close()


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
