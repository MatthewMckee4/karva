# /// script
# requires-python = ">=3.13"
# dependencies = ["psutil==7.2.2"]
# ///

"""Measure a real stdio server against a reproducible fixture-heavy workspace."""

import argparse
import hashlib
import json
import platform
import shutil
import statistics
import tempfile
import time
from pathlib import Path

import psutil
from zed_smoke import LspClient


def diagnostics(client, uri, version):
    """Wait for this edit's publication, preserving other protocol messages."""
    while True:
        for index, message in enumerate(client.notifications):
            params = message.get("params", {})
            if (
                message.get("method") == "textDocument/publishDiagnostics"
                and params.get("uri") == uri
                and params.get("version") == version
            ):
                client.notifications.pop(index)
                return params["diagnostics"]
        client.notifications.append(client.receive())


def measure(command, workspace, source, edits):
    """Keep process startup, first analysis and cached requests as separate samples."""
    elapsed = {}
    rss = {}
    with tempfile.TemporaryFile(mode="w+b") as stderr:
        started = time.perf_counter()
        client = LspClient(command, workspace, stderr)
        process = psutil.Process(client.process.pid)
        try:
            result = client.request(
                "initialize",
                {
                    "processId": None,
                    "workspaceFolders": [
                        {"uri": workspace.as_uri(), "name": "benchmark"}
                    ],
                    "capabilities": {
                        "workspace": {
                            "didChangeWatchedFiles": {"dynamicRegistration": True}
                        }
                    },
                },
            )
            elapsed["initialize_seconds"] = time.perf_counter() - started
            client.notify("initialized", {})
            registration = client.receive()
            assert registration.get("method") == "client/registerCapability"
            client.send({"id": registration["id"], "result": None})
            document = {"uri": (workspace / "test_0.py").as_uri()}
            started = time.perf_counter()
            client.notify(
                "textDocument/didOpen",
                {
                    "textDocument": {
                        **document,
                        "languageId": "python",
                        "version": 1,
                        "text": source,
                    }
                },
            )
            assert diagnostics(client, document["uri"], 1) == []
            elapsed["open_diagnostics_seconds"] = time.perf_counter() - started
            rss["after_open_bytes"] = process.memory_info().rss
            # Fixture position is identical in every generated test function.
            params = {
                "textDocument": document,
                "position": {"line": 0, "character": 20},
            }
            for temperature in ("first", "warm"):
                for method in ("completion", "definition"):
                    started = time.perf_counter()
                    response = client.request(f"textDocument/{method}", params)
                    elapsed[f"{temperature}_{method}_seconds"] = (
                        time.perf_counter() - started
                    )
                    assert response, (method, response)
                started = time.perf_counter()
                assert client.request("workspace/symbol", {"query": "test_"})
                elapsed[f"{temperature}_workspace_symbols_seconds"] = (
                    time.perf_counter() - started
                )
            rss["after_workspace_index_bytes"] = process.memory_info().rss
            elapsed["edit_diagnostics_seconds"] = []
            for version in range(2, edits + 2):
                started = time.perf_counter()
                client.notify(
                    "textDocument/didChange",
                    {
                        "textDocument": {**document, "version": version},
                        "contentChanges": [{"text": source + f"\n# edit {version}\n"}],
                    },
                )
                assert diagnostics(client, document["uri"], version) == []
                elapsed["edit_diagnostics_seconds"].append(
                    time.perf_counter() - started
                )
            rss["after_edits_bytes"] = process.memory_info().rss
            client.request("shutdown", None)
            client.notify("exit", None)
            client.process.stdin.close()
            assert client.process.wait() == 0
            return {"server": result.get("serverInfo"), "latency": elapsed, "rss": rss}
        finally:
            client.close()


def positive(value):
    """Reject empty workloads and measurements before launching a server."""
    number = int(value)
    if number < 1:
        raise argparse.ArgumentTypeError("must be greater than zero")
    return number


def main():
    """Write raw samples and medians; no latency or memory budget is guessed."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--server", type=Path, default=shutil.which("karva"))
    parser.add_argument("--modules", type=positive, required=True)
    parser.add_argument("--tests-per-module", type=positive, required=True)
    parser.add_argument("--iterations", type=positive, required=True)
    parser.add_argument("--edits", type=positive, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    if args.server is None:
        parser.error("pass --server or put karva on PATH")
    source = "".join(
        f"def test_case_{index}(sample):\n    assert sample\n\n"
        for index in range(args.tests_per_module)
    )
    conftest = "from karva import fixture\n\n@fixture\ndef sample():\n    return True\n"
    with tempfile.TemporaryDirectory(prefix="karva-lsp-benchmark-") as directory:
        workspace = Path(directory).resolve()
        (workspace / "karva.toml").write_text("")
        (workspace / "conftest.py").write_text(conftest)
        for index in range(args.modules):
            (workspace / f"test_{index}.py").write_text(source)
        samples = [
            measure(
                [str(args.server.resolve()), "server"], workspace, source, args.edits
            )
            for _ in range(args.iterations)
        ]
    latency = {}
    for key in samples[0]["latency"]:
        values = [sample["latency"][key] for sample in samples]
        if isinstance(values[0], list):
            values = [value for group in values for value in group]
        latency[key] = statistics.median(values)
    report = {
        "platform": platform.platform(),
        "python": platform.python_version(),
        "workload": {
            "modules": args.modules,
            "tests_per_module": args.tests_per_module,
            "source_bytes": len(source.encode()) * args.modules
            + len(conftest.encode()),
            "source_sha256": hashlib.sha256((conftest + source).encode()).hexdigest(),
            "edits_per_process": args.edits,
        },
        "cache_policy": "fresh process; OS filesystem cache uncontrolled",
        "memory_policy": "resident bytes sampled at milestones, not peak RSS",
        "median_latency": latency,
        "samples": samples,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(latency, indent=2))


if __name__ == "__main__":
    main()
