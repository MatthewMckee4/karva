# Contributing to Karva

Welcome, and thanks for contributing to Karva.

## Finding Ways to Help

[`contributor-friendly`](https://github.com/MatthewMckee4/karva/issues?q=is%3Aissue%20state%3Aopen%20label%3Acontributor-friendly)
issues are ready for community contributions.
[`bug`](https://github.com/MatthewMckee4/karva/issues?q=is%3Aissue%20state%3Aopen%20label%3Abug)
issues are also good candidates when the expected behavior is clear.

Comment before starting work so another contributor does not duplicate it and
the maintainer can confirm the issue is current. Discuss larger changes and
new features before opening a pull request; they can affect Karva's deliberately
small scope and long-term maintenance burden.

Use [GitHub issues](https://github.com/MatthewMckee4/karva/issues/new) for bug
reports, feature proposals, and documentation problems.

## The Basics

### Prerequisites

Karva development requires:

- The [Rust toolchain](https://www.rust-lang.org/tools/install).
- [uv](https://docs.astral.sh/uv/getting-started/installation/) for Python
  environments and tools.
- [just](https://just.systems/) for project recipes.

We recommend [nextest](https://nexte.st/) for faster Rust test runs:

```sh
cargo install cargo-nextest --locked
```

Install the optional repository hooks with:

```sh
uvx prek install
```

### Development

Run Karva from the repository root with a debug build:

```sh
cargo run test tests/test_add.py
```

The full test recipe builds a development wheel automatically. Some
integration tests compare Karva with pytest and require pytest in the Python
environment used by the test runner. Avoid relying on a project-local uv
environment for these tests.

### Project Structure

Karva uses a main process plus worker subprocesses. The `karva` binary
discovers and partitions tests, starts `karva-worker` processes, then combines
their cached results. Workers embed Python and execute tests. The binaries
communicate only through CLI arguments and the shared cache directory.

All Rust crates live under `crates/`:

- `karva` contains the main CLI and orchestration entry point.
- `karva_worker` embeds Python and runs assigned tests.
- `karva_cli` contains command types shared by both binaries.
- `karva_runner`, `karva_project`, `karva_collector`, and `karva_combine`
  implement discovery, partitioning, process management, and result
  aggregation.
- `karva_test_semantic` implements Python test execution and extensions.
- `karva_cache`, `karva_diagnostic`, `karva_logging`, `karva_metadata`, and
  `karva_static` provide shared infrastructure.
- `karva_python` builds the Python wheel and exposes the binaries.
- `karva_dev` contains documentation generators.
- `karva_benchmark` contains wall-time benchmarks and benchmark projects.

Python packaging files live under `python/`, documentation under `docs/`, and
CLI integration tests under `crates/karva/tests/it/`.

## Testing

Run the full suite:

```sh
just test
```

Pass standard `cargo nextest` or `cargo test` arguments for focused runs:

```sh
just test -p karva_cache
just test -p karva test_name
```

`just test` builds the wheel with `uvx maturin build`, then runs nextest when
available and falls back to `cargo test`.

Before opening a pull request, run relevant focused tests and the validation
sweep:

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
uvx prek run -a
```

GitHub Actions runs tests on Linux, macOS, and Windows. Platform-specific
behavior should have focused coverage where practical.

### Snapshot Tests

Prefer integration tests when behavior crosses crate, Python, worker, or CLI
boundaries. Command integration tests should use snapshots so diagnostics and
exit behavior remain visible.

Review every snapshot change before accepting it. Check for unexpected
`.snap.new` files before finishing, and never include unrelated snapshot
updates in a pull request.

## Documentation

Karva uses [Zensical](https://zensical.org/) for its documentation site. Build
the site with:

```sh
uv run --script scripts/prepare_docs.py
uv run --isolated --only-group docs zensical build
```

Run the documentation generator after changing configuration options, CLI
arguments, or environment variable definitions:

```sh
cargo run -p karva_dev generate-all
```

Files under `docs/reference/` and
`docs/configuration/configuration.md` are generated. Their headers identify
the source to edit.

## Benchmarks

The pull request benchmark workflow compares base and PR wheels against pinned
projects. It reports median wall time and peak memory in a PR comment. Keep
benchmark projects deterministic and pin external inputs.

Performance changes should include benchmark evidence when the impact is not
obvious from existing CI coverage.

## Opening a Pull Request

Use the pull request template, link relevant issues, and add labels that match
the affected area. Keep the pull request in draft while substantial work
remains.

### Summary

Explain what changed and why in concise prose. Include implementation details
only when reviewers need them to understand the design or trade-offs.

### Test Plan

State what you verified in one short sentence. If CI is the only remaining
validation, write `ci`.

Keep commits focused and use descriptive one-line subjects. Do not mix
formatter churn or unrelated cleanup with the change.

## Release Process

Releases are automated. Maintainers use
[`seal`](https://github.com/MatthewMckee4/seal) to update the version and
create a release branch:

```sh
seal bump alpha
seal bump <version>
```

Open a pull request from the generated branch. The release workflow handles
the remaining publication steps after merge.

## GitHub Actions

Actions must be pinned to full commit SHAs. After editing a workflow, run:

```sh
pinact run
```

Review generated workflow changes before committing them.
