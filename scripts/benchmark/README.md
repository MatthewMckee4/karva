# Karva vs Pytest Benchmark

This directory contains a benchmark script that compares the performance of karva and pytest on a large number of simple assertions.

## Setup

The project uses `uv` for dependency management. To set up, run:

```bash
uv sync
```

## Running the Benchmark

To run the benchmark cd into this directory and run:

```bash
uv sync --all-extras --no-install-project
uv pip install -e ../../
uv run main.py --num-tests 10000 (--run-test to generate graph)
```

Or use hyperfine:

```bash
hyperfine 'uv run karva test test_karva_many_assertions.py -q' --warmup 3 --runs 10
hyperfine 'uv run karva test test_pytest_many_assertions.py -q' --warmup 3 --runs 10
```

To compare karva and pytest:

```bash
hyperfine 'uv run karva test test_karva_many_assertions.py -q' 'uv run pytest test_pytest_many_assertions.py -q' --warmup 1 --runs 5;
```
