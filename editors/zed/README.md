# Karva for Zed

Karva adds its language server to Python files without replacing Pyright, ty,
or another Python language server.

Add Karva to the project's UV development dependencies:

```sh
uv add --dev karva
```

Install this directory with Zed's `Install Dev Extension` command, then enable
Karva in `settings.json`:

```json
{
  "languages": {
    "Python": {
      "language_servers": ["karva", "ty"]
    }
  }
}
```

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

Zed detects Python `test_*` functions itself and binds their gutter play icons
to pytest by default. A project can bind the same runnable tag to Karva with
`.zed/tasks.json`:

```json
[
  {
    "label": "Karva $ZED_CUSTOM_PYTHON_TEST_TARGET",
    "command": "uv",
    "args": [
      "run",
      "karva",
      "test",
      "$ZED_CUSTOM_PYTHON_TEST_TARGET"
    ],
    "cwd": "$ZED_WORKTREE_ROOT",
    "tags": ["python-pytest-method"]
  }
]
```

This file changes only the gutter action; the language server does not require
it.

## Development

Check and build the extension from the repository root:

```sh
cargo test --manifest-path editors/zed/Cargo.toml
cargo build --manifest-path editors/zed/Cargo.toml --target wasm32-wasip2
```
