## Boil the ocean

When planning, do not be afraid to suggest seemingly insane solutions. We are
rethinking what it means to make a Python test framework. Karva needs to be
cross-platform while having an amazing developer experience. It should feel
familiar to pytest users so developers and agents can transition easily. We
want execution to be extremely efficient, with memory and CPU usage as low as
possible, without compromising that developer experience.

## Every number needs a receipt

A limit without a measurement is a landmine. Before writing any number (a
maximum test count, a byte cap, a timeout), measure the real thing first, then
size it as a tripwire. Capacity is free until touched (reserve big, commit
lazily, never zero an arena eagerly), so be generous. If normal use hits a
budget, the budget is wrong. Remeasure and update the receipt.

## A limit developers can hit is a limit they must see

Developers will not read our code. Their agents read our errors. An agent can
fix a named limit with the requested and allowed values. It cannot fix a blank
window. Every budget failure names the budget, the limit, and the request: at
validation time if knowable there, loudly at runtime if not. A silent budget is
worse than no budget.

## Fight for the "obvious" solution

Measure twice, cut once: understand the problem fully before building, because
cleverness is what gets written when you have not. The biggest simplicity win
is refusing to solve problems we do not have. Good code is the simplest thing
that delivers full functionality and performance, with nothing traded away and
nothing bolted on. Push back when you see a more obvious way.

# Karva Repository

This repository contains Karva, a Python test framework implemented as a Rust
workspace with PyO3. Rust crates use the `karva_*` naming convention and live
under `crates/`.

## Architecture

Karva runs tests through a main `karva` process and `karva-worker`
subprocesses. The binaries do not link against each other. They communicate
through CLI arguments and a shared cache directory, and only the worker embeds
Python.

## Code Review Rules

Be deliberately nitpicky. Report bugs, regressions, architectural and
maintenance risks, weak tests, unclear code, unnecessary complexity, and
meaningful consistency issues. Number findings, order them by severity, cite
files and lines, and distinguish blockers from improvements.

## Development

- Write `Karva` for the project and `karva` for the executable or package name.
- Always invoke Karva as `uv run karva` in documentation commands and examples.
- Prefer narrow visibility because this workspace is generally its own
  consumer; use `pub` when another workspace crate genuinely needs an item.
- Keep Rust imports at the top of files and prefer short imports.
- Document production crates and non-obvious functions, structs, enums, traits,
  and fields with Rust doc comments. Add module docs when a module has an
  architectural role or boundary that its name and contents do not make clear.
  Explain contracts: purpose, invariants, units, side effects, failure behavior,
  and architectural role. Do not restate names, types, signatures, or
  implementation steps; omit a comment when those already communicate
  everything. Separate documented struct fields and enum variants with blank
  lines. Update or remove stale comments when behavior changes.
- Avoid `panic!`, `unreachable!`, `.unwrap()`, unsafe code, and Clippy ignores.
  Encode constraints in the type system.
- Prefer `if let` and let chains for fallibility.
- Use `#[expect(...)]` rather than `#[allow(...)]` when suppressing a lint.

## Tests

- Add focused tests when existing coverage does not establish changed behavior.
- Prefer integration tests under `crates/karva/tests/it/` for behavior crossing
  crate, Python, worker, or CLI boundaries.
- Snapshot command exit code, stdout, and stderr together. Do not call
  `.output()` only to assert success.
- Use `#[rstest]` with `#[values(...)]` instead of loops for repeated cases.
- Never edit snapshots manually. Regenerate them, review every changed snapshot,
  and check for `.snap.new` files.
- Use separate `#[cfg(unix)]` and `#[cfg(not(unix))]` snapshots for
  platform-dependent output.

## Verification

- Focused tests: `just test -p <crate> [test_name]`.
- Full suite: `just test`. It builds the Python wheel first and uses nextest
  when installed, otherwise `cargo test`.
- Do not run `cargo clippy --workspace --all-targets --all-features -- -D warnings`
  locally; let CI run the full workspace Clippy check.
- Run Karva with debug builds: `cargo run test tests/test_add.py`.
- After configuration, CLI, environment variable, or reference changes, run
  `cargo run -p karva_dev generate-all` and review generated files.
- After workflow changes, run `pinact run`; actions must use full commit SHAs.
- During iteration, run `uvx prek run --files <paths>`. Before finishing, run
  `uvx prek run -a`.

## Contributor Workflow

See `CONTRIBUTING.md` for documentation and pull requests.
