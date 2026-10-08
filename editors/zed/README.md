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

The extension uses an explicit `lsp.karva.binary.path`, then `karva server` from
the worktree PATH, then the project version through `uv run karva server`.
Install Karva in the project when using uv. If no command is available, the
extension reports how to configure a local installation.

For a debug Karva binary, configure the server subcommand explicitly:

```json
{
  "lsp": {
    "karva": {
      "binary": {
        "path": "/path/to/target/debug/karva",
        "arguments": ["server"],
        "env": {"RUST_LOG": "debug"}
      }
    }
  }
}
```

Explicit and PATH binaries default to the `server` argument. Binary arguments
and environment overrides apply to every resolution mode.

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

## Fixture inlay hints

Enable Zed's inlay hints to see each injected fixture's scope and provider next
to test parameters, fixture dependencies, and `usefixtures` names:

```json
{
  "inlay_hints": {
    "enabled": true,
    "show_other_hints": true
  }
}
```

Hints follow fixture overrides and unsaved provider edits. Unknown fixtures and
ordinary Python parameters are left to diagnostics and the Python language server.

## Workspace symbol search

Use the editor's workspace symbol search to find Karva tests and fixtures across
all project folders. Search accepts case-insensitive characters in order;
fixture results use their public names. Unsaved Python buffers override saved
files, and nested Karva projects use their own test prefixes.

## Fixture call hierarchy

With the cursor on a fixture declaration or reference, use `call hierarchy: show incoming calls` to see tests and fixtures that consume that provider. Use
`call hierarchy: show outgoing calls` to see its direct fixture dependencies.
Entries marked `(fixture)` distinguish fixture dependencies from ordinary
Python calls. The picker can navigate deeper through either direction, including providers
in ancestor `conftest.py` files. Fixture overrides remain separate; ordinary
Python calls and built-in fixtures are left to the Python language server.

When a Python language server also supports call hierarchy, start from an
injected fixture parameter. Zed currently uses only the first prepared result
at a declaration, which can select the ordinary Python hierarchy instead.

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

## Language-server logs

Karva writes language-server warnings, errors, and panic backtraces to stderr,
which Zed captures in its language-server logs. Set `RUST_LOG=karva_language_server=debug`
in the environment that starts Zed to enable debug tracing. Panics also produce
an editor error message; a failed request receives an internal-error response.

## Development

Check and build the extension from the repository root:

```sh
rustup target add wasm32-wasip2
cargo test --manifest-path editors/zed/Cargo.toml
cargo build --manifest-path editors/zed/Cargo.toml --target wasm32-wasip2
```

### Code-lens client contract

The language server returns resolved lenses for collected test declarations.
Editor integrations can implement these client-side command handlers:

`karva.runTests` receives one object with `cwd`, `program`, and `args`. Save
modified buffers, then launch the program in that working directory with the
argument array, without combining arguments into a shell string. The payload
uses `uv run karva test`, a project-relative file or function selector, and the
initialization profile when supplied.

`karva.copyTestId` receives one string: the runtime-qualified function ID,
including its dotted module name. Copy it to the clipboard.

Requesting lenses does not run a process. Clients without these handlers can
continue using the gutter tasks above; this extension does not register lens
command handlers yet.
