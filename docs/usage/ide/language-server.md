# Language server

Karva releases include a standalone `karva-language-server` executable. It
speaks the Language Server Protocol (LSP) over standard input and standard
output, so any LSP client can use it without installing Karva from a source
checkout.

## Installation

Download the archive for your operating system and architecture from the
[Karva releases](https://github.com/MatthewMckee4/karva/releases). Extract the
executable, place it on `PATH`, and verify the installation:

```console
$ karva-language-server --version
karva 0.0.1-alpha.11
```

Release archives include a `.sha256` file. Verify it before installing the
executable. Unix archives are named `karva-language-server-<target>.tar.gz`;
Windows archives are named `karva-language-server-<target>.zip`.

## Generic LSP client setup

Configure your editor's generic LSP client to start `karva-language-server`
with no arguments. The client must use stdio transport and send the normal LSP
`initialize` request. If the executable is not on `PATH`, configure its
absolute path instead.

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

## Logs and startup failures

The server never writes logs to stdout because stdout is reserved for LSP
messages. Set `logLevel` to `error`, `warn`, `info`, `debug`, or `trace` in the
LSP `initialize` options to control stderr logging. Set `logFile` to a path
when the editor hides stderr; the path's parent directory must already exist.
For example:

```json
{
  "logLevel": "debug",
  "logFile": "/tmp/karva-language-server.log"
}
```

If startup fails, the process exits non-zero and reports the cause on stderr.
The most common fixes are checking the executable path, ensuring the archive
matches the host architecture, and starting the server with no command-line
arguments. A client that cannot start the process should show stderr and the
process exit status in its LSP diagnostics.

## Command-line contract

`karva-language-server` starts the stdio server. `--version` (or `-V`) prints
the executable name and release version, and `--help` (or `-h`) prints the
available options. Any other argument is rejected with a non-zero exit status.
