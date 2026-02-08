# Contributing to Karva

Thank you for considering contributing to Karva! We welcome contributions from everyone.

Not only do we aim to make Karva a better tool for everyone, but we also aim to make the contributing process as smooth and enjoyable as possible.

So, if you come across any issues or have suggestions for improving development in the karva repo, please [open an issue](https://github.com/karva-dev/karva/issues/new).

## Reporting Issues

If you encounter any issues or have suggestions for improvements, please [open an issue](https://github.com/karva-dev/karva/issues/new).

## The Basics

For small changes (e.g., bug fixes), feel free to submit a PR.

For larger changes, consider creating an issue outlining your proposed change.

If you have suggestions on how we might improve the contributing documentation, let us know!

## Architecture

Karva uses a **main-process + worker-subprocess** execution model. When you run `karva test`, the main process (`karva`) collects test files, partitions them across workers, then spawns one or more `karva-worker` subprocesses to actually execute the tests. Each worker writes its results to a shared cache directory, and the main process aggregates results when all workers finish.

### Crate Map

**Binaries:**

- `karva` — Main CLI binary. Parses args, discovers test files, partitions work, spawns workers, aggregates results.
- `karva_worker` — Worker subprocess binary. Receives a subset of test files, runs them, writes results to cache.

**Shared libraries (used by both binaries):**

- `karva_cli` — Shared CLI types (`SubTestCommand`, `Verbosity`, etc.), the bridge between main and worker.
- `karva_cache` — Cache directory layout, result serialization, duration tracking.
- `karva_static` — Environment variable constants, `max_parallelism()`.
- `karva_metadata` — Project configuration (`ProjectSettings`), config file parsing.
- `karva_diagnostic` — Test result types (`TestRunResult`), diagnostic reporting.
- `karva_logging` — Tracing setup, `Printer`, colored output control, duration formatting.
- `karva_python_semantic` — Python version detection, AST-level semantic types.

**Main-process only:**

- `karva_runner` — Orchestration: worker spawning, partitioning, parallel collection.
- `karva_project` — Project metadata, test path resolution, path utilities.
- `karva_collector` — File-level test collection (parsing Python files for test functions).
- `karva_combine` — Result combination and summary output.

**Worker-process only:**

- `karva_test_semantic` — Core test execution library: discovery, context, extensions, PyO3 runner.

**Infrastructure / Build:**

- `karva_python` — PyO3 `cdylib`, the Python wheel entry point. Wraps both `karva` and `karva_worker`.
- `karva_macros` — Procedural macros.
- `karva_dev` — Dev tools (CLI reference generation, etc.).

**Dev / Testing:**

- `karva_benchmark` — Wall-time benchmarks using divan, including real-world project definitions.

### Key Design Decisions

- **Binaries don't depend on each other.** `karva` and `karva_worker` communicate only through the filesystem (cache directory) and CLI arguments.
- **Shared types live in `karva_cli`.** Both binaries depend on `karva_cli` for common command-line types like `SubTestCommand`.
- **The worker embeds a Python interpreter.** `karva_test_semantic` uses PyO3 to attach to Python for test execution, while the main process only needs Python for the wheel packaging.

### Prerequisites

Karva is written in Rust. You can install the [Rust Toolchain](https://www.rust-lang.org/tools/install) to get started.

You can optionally install prek hooks to automatically run the validation checks when making a commit:

```bash
uv tool install prek
prek install
```

### Development

Note, you can use [just](https://github.com/casey/just) to run some useful commands.

To run the cli on a test file, run:

```bash
cargo run test tests/test_add.py
```

Annoyingly, you need a global python with pytest installed.

We have had many issues with local development using `uv` virtual environments with pytest installed, but this does not always work well.

If you want to run the tests, you need to build a wheel every time, so you need to run the following:

```bash
maturin build
cargo nextest run 
```

Or simply, with just, run:

```bash
just test
```

### Documentation

We use zensical to build the documentation.

```bash
uv run -s scripts/prepare_docs.py
uv run --isolated --only-group docs zensical build
```

## Release Process

Currently, everything is automated for releasing a new version of Karva.

First, install [seal](https://github.com/MatthewMckee4/seal), then bump the version with the following:

```bash
# Bump the alpha version
seal bump alpha

# Bump to a version
seal bump <version>
```

This will create a new branch and make a commit, so you just need to make a pull request.

## GitHub Actions

If you are updating github actions, ensure to run `pinact` to pin action versions.

```bash
pinact run
```
