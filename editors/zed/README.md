# Karva for Zed

Karva adds its language server to Python files without replacing Pyright, ty,
or another Python language server.

Add Karva to the project's UV development dependencies:

```sh
uv add --dev karva
```

Install this directory with Zed's `Install Dev Extension` command. Zed starts
the Karva language server for Python automatically.

The extension starts the project version with `uv run karva server`. Keep `uv`
on Zed's worktree `PATH`; no language-server binary path or release download is
needed.

Initialization and workspace settings remain available under `lsp.karva`:

```json
{
  "lsp": {
    "karva": {
      "initialization_options": {
        "profile": "ci",
        "pythonVersion": "3.13"
      },
      "settings": {}
    }
  }
}
```

## Gutter test runs

Karva exposes project, file, top-level function, and doctest
targets through `experimental/runnables`. Zed uses them as native gutter tasks
when it supports capability-advertised LSP runnables. No `.zed/tasks.json` or
language setting is required.

Released Zed versions that do not yet discover runnable-capable Python language
servers continue to show the built-in pytest tasks.

## Development

Check and build the extension from the repository root:

```sh
cargo test --manifest-path editors/zed/Cargo.toml
cargo build --manifest-path editors/zed/Cargo.toml --target wasm32-wasip2
```
