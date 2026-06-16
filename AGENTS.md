# Karva Repository

Karva is a Rust workspace that builds a Python test framework through PyO3. Read `CONTRIBUTING.md` before changing files; it has the current architecture map, setup commands, benchmark notes, docs build commands, and release process.

## Running Tests

Run the full suite through the project recipe:

```sh
just test
```

Pass `cargo nextest` or `cargo test` arguments through `just test` when a narrower run is enough during iteration:

```sh
just test -p karva_cache
```

`just test` builds the wheel with `uvx maturin build`, then uses `cargo nextest run` if `cargo-nextest` is installed and falls back to `cargo test`.

Use debug builds while developing. To run the CLI against a test file:

```sh
cargo run test tests/test_add.py
```

Run Clippy with the same strictness as CI:

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Run `uvx prek run -a` at the end of every task. During iteration, use `uvx prek run --files <path1> <path2>` with every changed file to keep hook runs independent of staged state.

## Snapshots And Generated Files

Prefer integration tests, for example under `it/...`, over unit tests when the behavior crosses crate or CLI boundaries. Command integration tests should use snapshots.

After updating snapshots, always review what changed before accepting them. Check for pending `.snap.new` files before finishing.

Run `cargo dev generate-all` after changing configuration options, CLI arguments, environment variable definitions, or anything else that feeds generated reference docs under `docs/`.

## Development Guidelines

- All behavior changes must be tested. If you did not run the relevant tests, the change is not done.
- Look for existing utilities and local patterns before writing new code.
- Keep visibility narrow by default, but make an item public when another workspace crate needs it and that is the cleaner implementation.
- Keep Rust imports at the top of the file, not locally inside functions.
- Avoid `panic!`, `unreachable!`, `.unwrap()`, unsafe code, and Clippy ignores. Encode constraints in the type system instead.
- Prefer `if let` for fallibility, and prefer let chains over nested `if let` when it improves readability.
- Prefer `#[expect()]` over `#[allow()]` when a Clippy lint must be suppressed.
- Prefer short imports over fully qualified paths for readability.
- Use comments to explain invariants or unusual decisions, not to narrate code.
- Avoid redundant comments and section separators in tests.
- Prefer function comments over inline comments.
- Consider whether a change needs a docs update under `docs/`. New flags, public APIs, removed features, changed defaults, config changes, CLI changes, and environment variable changes usually need one.

## Pull Requests

Always use the pull request template and add labels. Write the description in concise prose paragraphs, with code examples only when they help the reviewer.
