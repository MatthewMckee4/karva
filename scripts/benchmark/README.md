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
uv run main.py --iterations 5 --num-tests 10000 --run-test
```
