# Contributing to Karva

Thanks for your interest in contributing to Karva. Contributions of all kinds
are welcome, and we try to keep the development process as smooth as possible.

If you hit a bug, have a feature idea, or want to suggest an improvement to
the contributing docs themselves, please
[open an issue](https://github.com/MatthewMckee4/karva/issues/new).

For small changes like bug fixes, feel free to jump straight to a pull request.
For anything larger, it's usually worth opening an issue first to discuss the
approach.

If you are wanting to attempt to tackle an issue that is already open, please
leave a comment letting me know you would like to work on it. There's only one
person working on this project right now, so issues may be out of date and I
don't want you to work on something that doesn't align with the goals for the
project, so let's have a chat about the issue first. Thank you in advance.

## Architecture

Karva runs tests using a **main process plus worker subprocesses**. When you
run `karva test`, the main `karva` process discovers the test files, partitions
them across workers, and spawns one or more `karva-worker` subprocesses to
actually execute the tests. Each worker writes its results into a shared cache
directory, and the main process aggregates everything once the workers finish.

The two binaries never link against each other. They communicate only through
CLI arguments and the cache directory on disk. Shared types live in
`karva_cli`, which both binaries depend on. Only the worker embeds a Python
interpreter via PyO3 — the main process only touches Python for wheel
packaging.

### Crate Map

The two binaries:

- `karva` — main CLI. Parses arguments, discovers test files, partitions work, spawns workers, and aggregates results.
- `karva_worker` — worker subprocess. Receives a subset of test files, runs them, and writes results to the cache.

Libraries shared between both binaries:

- `karva_cli` — shared CLI types (`SubTestCommand`, `Verbosity`, etc.), the bridge between main and worker.
- `karva_cache` — cache directory layout, result serialization, and duration tracking.
- `karva_static` — environment variable constants and `max_parallelism()`.
- `karva_metadata` — project configuration (`ProjectSettings`) and config file parsing.
- `karva_diagnostic` — test result types (`TestRunResult`) and diagnostic reporting.
- `karva_logging` — tracing setup, `Printer`, colored output control, and duration formatting.
- `karva_python_semantic` — Python version detection and AST-level semantic types.

Used only by the main process:

- `karva_runner` — orchestration: worker spawning, partitioning, and parallel collection.
- `karva_project` — project metadata, test path resolution, and path utilities.
- `karva_collector` — file-level test collection (parsing Python files for test functions).
- `karva_combine` — result combination and summary output.

Used only by the worker process:

- `karva_test_semantic` — the core test execution library: discovery, context, extensions, and the PyO3 runner.

Infrastructure and tooling:

- `karva_python` — the PyO3 `cdylib` that produces the Python wheel and wraps both `karva` and `karva_worker`.
- `karva_macros` — procedural macros.
- `karva_dev` — dev tools such as CLI reference generation.
- `karva_benchmark` — wall-time benchmarks that run `karva test` against pinned synthetic and open-source benchmark projects.

### Prerequisites

Karva is written in Rust. You can install the [Rust Toolchain](https://www.rust-lang.org/tools/install) to get started.

You will also need to install [maturin](https://github.com/PyO3/maturin) to build the Python wheel:

```bash
uv tool install maturin
```

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

### Benchmarks

Karva has a pinned wall-time benchmark suite for CI and local comparison. It
includes the synthetic `karva-benchmark-1` project plus selected open-source
projects that Karva can run cleanly today. The JSON benchmark report is the
repeatable CI artifact; CodSpeed wall-time upload is best-effort trend
reporting because CodSpeed macro runners require GitHub organization ownership.

```bash
just benchmark --samples 5 --output target/benchmark-results/karva-benchmark.json
```

To run a single project:

```bash
just benchmark --project packaging --samples 5
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
