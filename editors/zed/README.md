# Karva for Zed

Karva adds its Python language server to Zed. It does not replace or bundle a
Python grammar, so it can run beside Pyright, basedpyright, ty, or another
Python language server.

Install this directory as a development extension from Zed's Extensions page,
then enable Karva in `settings.json`:

```json
{
  "languages": {
    "Python": {
      "language_servers": ["karva", "pyright"]
    }
  }
}
```

Zed resolves `karva-language-server` in this order: `lsp.karva.binary.path`,
the worktree `PATH`, then the matching asset from the latest Karva GitHub
release. Downloaded releases are cached in Zed's extension work directory.

```json
{
  "lsp": {
    "karva": {
      "binary": {
        "path": "/path/to/karva-language-server",
        "arguments": [],
        "env": {"KARVA_LOG": "debug"}
      },
      "initialization_options": {},
      "settings": {}
    }
  }
}
```

Keep `karva` in the language-server list to run it alongside another server.
Prefix a server with `!` to disable it for one language, for example
`["pyright", "!karva"]`.

For local testing, use Zed's `Install Dev Extension` command and select this
directory. The extension's Rust adapter can be checked with:

```sh
cargo test --manifest-path editors/zed/Cargo.toml
cargo check --manifest-path editors/zed/Cargo.toml --target wasm32-wasip2
```
