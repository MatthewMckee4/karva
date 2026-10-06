# Karva for Zed

Karva adds its language server to Python files without replacing Pyright, ty,
or another Python language server.

Add Karva to the project's UV development dependencies:

```sh
uv add --dev karva
```

From the repository root, install the extension build target for the pinned
Rust toolchain before installing the development extension:

```sh
rustup target add wasm32-wasip2
```

A target installed for a different toolchain (including `stable`) does not
satisfy this requirement. Install this directory with Zed's `Install Dev Extension` command, then enable
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

## Python snippets

Type a trigger in a Python buffer and accept its completion with Enter. Tab then
moves between editable placeholders. Snippets use module-level declarations
and Karva's public APIs, with inline snapshots by default.

| Trigger | Expansion |
| --- | --- |
| `karva_test` | Module-level test |
| `karva_fixture` | Sync fixture |
| `karva_async_fixture` | Async fixture |
| `karva_teardown` | Generator fixture with cleanup |
| `karva_auto_use` | Fixture with `auto_use=True` |
| `karva_parametrize` | Parametrized test with stable case IDs |
| `karva_tag` | Custom-tagged test |
| `karva_expect_fail` | Expected failure with a reason |
| `karva_snapshot` | Inline text snapshot |
| `karva_json_snapshot` | Inline JSON snapshot |
| `karva_doctest` | Function with a doctest |

Doctests need `uv run karva test --doctest-modules` or the corresponding profile
setting. Imports appear in standalone snippets; keep one `import karva` at the
top of the module when combining expansions.

Validate snippet expansions against an installed Karva wheel:

```sh
just test -p karva zed_snippets
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
    "save": "all",
    "tags": ["python-pytest-method"]
  }
]
```

The task saves all modified files before running, including fixture providers
such as `conftest.py`, so Karva executes the code shown in the editor.

Karva runs top-level test functions. Zed also detects class methods with the
same runnable tag; their gutter actions are not supported by Karva and fail
with an unsupported-selector error. Move Karva tests to top-level functions,
or keep Zed's default pytest task for projects using test classes. Functions
with a custom Karva prefix are not detected by Zed's native `test_*` query; run
them from the task picker with an exact selection, for example:

```sh
uv run karva test tests/test_example.py::check_example
```

This file changes only the gutter action; the language server does not require
it.

## Development

Check and build the extension from the repository root:

```sh
rustup target add wasm32-wasip2
cargo test --manifest-path editors/zed/Cargo.toml
cargo build --manifest-path editors/zed/Cargo.toml --target wasm32-wasip2
```
