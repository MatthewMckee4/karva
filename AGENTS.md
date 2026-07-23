# Karva Repository

This repository contains Karva, a Python test framework implemented as a Rust
workspace with PyO3. Rust crates use the `karva_*` naming convention and live
under `crates/`.

Karva runs tests through a main `karva` process and `karva-worker`
subprocesses. The binaries do not link against each other. They communicate
through CLI arguments and a shared cache directory, and only the worker embeds
Python.

## Code Review Rules

When reviewing a branch or pull request, be deliberately nitpicky. Report not
only bugs and regressions, but also architectural and maintenance risks, weak
test coverage, unclear code, unnecessary complexity, and meaningful style or
consistency issues. Order findings by severity, cite files and lines, and
distinguish blockers from non-blocking improvements. Number each review point
for easy reference in subsequent review discussion.

## Running Tests

Run all tests:

```sh
just test
```

Run tests for a specific crate:

```sh
just test -p karva_cache
```

Run a specific test:

```sh
just test -p karva test_name
```

`just test` builds the Python wheel with `uvx maturin build` before running
the Rust test suite.

### Fallback Without Nextest

The `just test` recipe uses `cargo nextest run` when `cargo-nextest` is
installed and falls back to `cargo test` automatically. Arguments shown above
work with either runner.

### Snapshot Updates

After running tests, always review every snapshot that was added or updated.
Check for pending `.snap.new` files if affected tests use Karva snapshots.

Do not edit snapshot files or inline snapshot bodies manually. Regenerate them
by running the relevant tests or Karva snapshot command, then review the
generated diff. Do not accept unrelated snapshot changes.

## Running Clippy

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

## Running Debug Builds

Use debug builds, not `--release`, while developing. Release builds lack debug
assertions and take longer to compile.

Run Karva against a test file:

```sh
cargo run test tests/test_add.py
```

## Generated Documentation

Reference files under `docs/reference/` and
`docs/configuration/configuration.md` are generated. Their headers identify
the source file to change.

After changing configuration options, CLI arguments, environment variable
definitions, or their reference documentation, regenerate all references:

```sh
cargo run -p karva_dev generate-all
```

Review the generated diff before committing it.

## GitHub Actions

Actions must be pinned to full commit SHAs. After changing a workflow, run:

```sh
pinact run
```

## Development Guidelines

- All significant changes must be tested. Add or update focused tests for
  behavior changes when existing coverage does not establish the intended
  behavior.
- Look for an existing test file before creating a new one.
- Get the relevant tests to pass. If the tests were not run, the change is not
  done.
- Follow existing code style. Check neighboring files for patterns.
- Prefer integration tests under `crates/karva/tests/it/` when behavior crosses
  crate, Python, worker, or CLI boundaries. Command integration tests should
  use snapshots.
- Keep changes focused. Do not expand the task to unrelated issues.
- Before writing significant new code, look for existing utilities or
  mechanisms that solve the problem. Prefer fixing the underlying
  architectural problem over adding a localized workaround. Ask for guidance
  when the larger change is unclear.
- Prefer narrow visibility because this workspace is generally its own
  consumer. Do not add workarounds solely to avoid `pub`; make an item public
  when another workspace crate needs it and that produces the cleaner design.
- Keep Rust imports at the top of the file, never locally inside functions.
- Avoid `panic!`, `unreachable!`, `.unwrap()`, unsafe code, and Clippy ignores.
  Encode constraints in the type system.
- Prefer `if let` for fallibility. Prefer let chains over nested `if let`
  statements when they reduce indentation.
- If a Clippy lint must be suppressed, prefer `#[expect()]` over `#[allow()]`.
  Delete unused code instead of suppressing dead-code warnings.
- Prefer short imports over fully qualified paths.
- Do not use comments to narrate code. Use them to explain invariants and why
  unusual decisions were made. Prefer plain language that makes sense without
  prior context.
- Avoid redundant comments and section separators in tests. Prefer function
  comments over inline comments.
- Consider whether public APIs, flags, defaults, configuration, environment
  variables, or CLI changes require documentation updates.
- Run `uvx prek run --files <path1> <path2>` during iteration and pass every
  changed file. Run `uvx prek run -a` before finishing.

## Pull Requests

Use the pull request template and add relevant labels. Keep the summary and
test plan concise. Write descriptions as prose, not bullet lists or
checklists. Explain what changed and why; include implementation details only
when reviewers need them.

Use a descriptive one-line commit subject by default. Never add an AI tool as
an author or co-author.
