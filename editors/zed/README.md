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
Each download uses a unique install directory. The verified directory itself
becomes an immutable cache entry. Successful installs retain their verified
binary, archive, checksum, and completion marker. Valid installs remain
available for reuse; older release directories remain until manually removed.

If Zed stops or an install fails, its partial directory and any downloaded
files remain. The error names that directory. After stopping Zed and confirming
no Karva install is running, inspect and remove only that reported directory:

```sh
rm -rf -- '<reported-install-directory>'
```

On Windows PowerShell:

```powershell
Remove-Item -LiteralPath '<reported-install-directory>' -Recurse -Force
```

The extension never deletes incomplete, invalid, or unrelated directories.

Release downloads need Zed's `download_file` capability. Add this to your
user `settings.json` (or keep an equivalent broader grant):

```json
{
  "granted_extension_capabilities": [
    {
      "kind": "download_file",
      "host": "github.com",
      "path": ["MatthewMckee4", "karva", "**"]
    }
  ]
}
```

The extension manifest requests this same GitHub repository scope. Zed still
requires the user grant; without it, automatic downloads fail with a
`download_file` capability error. Explicit `lsp.karva.binary.path` and a
worktree `PATH` binary do not download anything.

Automatic release selection covers Linux `i686`, `x86_64`, and `aarch64`
GNU assets; Windows `i686`, `x86_64`, and `aarch64` MSVC assets; and macOS
`x86_64` and `aarch64` assets. Zed's extension API reports only operating
system and CPU architecture, not libc, so Linux musl and ARMv7 assets cannot
be selected automatically. Use `lsp.karva.binary.path` for those targets.

```json
{
  "lsp": {
    "karva": {
      "binary": {
        "path": "/path/to/karva-language-server",
        "arguments": [],
        "env": {}
      },
      "initialization_options": {
        "logLevel": "debug"
      },
      "settings": {}
    }
  }
}
```

Keep `karva` in the language-server list to run it alongside another server.
Prefix a server with `!` to disable it for one language, for example
`["pyright", "!karva"]`.

The extension API does not provide a project-configuration discovery hook.
Karva therefore cannot auto-enable itself from `pyproject.toml` or
`karva.toml`; enable `karva` in the Python language-server list. Once started,
the server receives the Zed workspace and performs its normal Karva project
discovery.

For local development, build the server from the repository root:

```sh
cargo build -p karva_language_server
```

Set `lsp.karva.binary.path` to the absolute repository path plus
`target/debug/karva-language-server`:

```json
{
  "lsp": {
    "karva": {
      "binary": {
        "path": "<karva-repo>/target/debug/karva-language-server"
      }
    }
  }
}
```

Replace `<karva-repo>` with the absolute checkout path. On Windows use
`target/debug/karva-language-server.exe`. Then run Zed's `Install Dev Extension`
command and select `editors/zed`.

The extension's Rust adapter can be checked with:

```sh
cargo test --manifest-path editors/zed/Cargo.toml
cargo build --manifest-path editors/zed/Cargo.toml --target wasm32-wasip2
```
