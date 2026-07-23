# Karva Repository

Karva is a Rust workspace that builds a Python test framework through PyO3.
Read `CONTRIBUTING.md` before changing files.

Karva runs tests through a main `karva` process and `karva-worker`
subprocesses. The binaries do not link against each other; they communicate
through CLI arguments and a shared cache directory. Only the worker embeds
Python.

## Code Review Rules

Review branches and pull requests critically. Report bugs, regressions,
maintenance risks, weak coverage, unclear code, unnecessary complexity, and
meaningful consistency issues. Order findings by severity, cite files and
lines, and distinguish blockers from optional improvements.

## Running Tests

Run the full suite through the project recipe:

```sh
just test
```

Pass `cargo nextest` or `cargo test` arguments through `just test` for focused
runs:

```sh
just test -p karva_cache
just test -p karva test_name
```

`just test` builds the Python wheel with `uvx maturin build`, then uses
`cargo nextest run` when `cargo-nextest` is installed and falls back to
`cargo test`.

Prefer integration tests under `crates/karva/tests/it/` when behavior crosses
crate, Python, worker, or CLI boundaries. Command integration tests should use
snapshots.

### Snapshot Updates

Review every added or updated snapshot before accepting it. Check for pending
`.snap.new` files before finishing. Do not accept unrelated snapshot changes.

## Running Clippy

Run Clippy with the same strictness as CI:

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Running Debug Builds

Use debug builds while developing. Run the CLI against a test file with:

```sh
cargo run test tests/test_add.py
```

## Generated Files

Run the generator after changing configuration options, CLI arguments,
environment variable definitions, or their reference documentation:

```sh
cargo run -p karva_dev generate-all
```

Generated files under `docs/reference/` and
`docs/configuration/configuration.md` identify their source files in a header.
Change the source, then regenerate.

Run `pinact run` after changing GitHub Actions workflows.

## Development Guidelines

- Test every behavior change. If the relevant tests were not run, the change
  is not done.
- Look for existing utilities and neighboring patterns before writing new
  code.
- Keep changes focused. Do not refactor unrelated code.
- Keep visibility narrow by default, but use `pub` when another workspace
  crate needs it and that is the cleaner design.
- Keep Rust imports at the top of the file.
- Avoid `panic!`, `unreachable!`, `.unwrap()`, unsafe code, and Clippy ignores.
  Encode constraints in the type system.
- Prefer `if let` for fallibility and let chains over nested `if let` when they
  improve readability.
- Prefer `#[expect()]` over `#[allow()]` when a lint must be suppressed.
- Prefer short imports over fully qualified paths.
- Use comments for invariants and unusual decisions, not narration.
- Avoid redundant comments and section separators in tests. Prefer function
  comments over inline comments.
- Consider whether public APIs, flags, defaults, configuration, environment
  variables, or CLI changes require a docs update.
- Run `uvx prek run --files <path1> <path2>` during iteration with every
  changed file, then run `uvx prek run -a` before finishing.

## Pull Requests

Use the pull request template and add relevant labels. Keep the summary and
test plan concise. Explain what changed and why; include implementation detail
only when reviewers need it.
