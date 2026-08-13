# Language server

Karva includes a Language Server Protocol (LSP) server in the `karva`
executable. Editors start it through the same project environment used for
tests:

```console
$ uv run karva server
```

The command communicates over standard input and standard output. It runs
until the editor sends the normal LSP shutdown and exit messages.

## Installation

Add Karva to the project's development dependencies and sync the environment:

```console
$ uv add --dev karva
$ uv sync
```

Verify that the server command is available without starting it:

```console
$ uv run karva server --help
```

## Generic LSP client setup

Configure the editor's generic LSP client to run `uv` with the arguments
`run`, `karva`, and `server`. Start the command in the project directory so UV
selects the project's environment. The client must use stdio transport and send
the normal LSP `initialize` request.

The server selects the current working directory when the client does not send
workspace folders. When workspace folders are sent, each folder is treated as
a separate Karva project. The nearest project configuration is discovered for
each folder, and the server keeps the workspace folder association across
configuration reloads.

The server discovers Karva configuration from the nearest `pyproject.toml`
(`tool.karva`) or `karva.toml`; an explicit `karva.toml` takes precedence when
both are present. The server registers a `workspace/didChangeWatchedFiles`
watcher when the client supports dynamic file watching. After changing
configuration, clients without that capability can use `karva.refreshWorkspace`
through `workspace/executeCommand` to refresh project state without restarting
the server.

## Startup failures

The server never writes logs to stdout because stdout is reserved for LSP
messages. If startup fails, `uv run karva server` exits non-zero and reports the
cause on stderr. Check that UV is available, the project environment is synced,
and the editor starts the command in the project directory.

## Command-line contract

`uv run karva server` starts the stdio server. `uv run karva server --help`
prints the available options. Other arguments are rejected before the server
starts.
