# Karva (0.0.9)

A Python test framework, written in Rust.

<div align="center">
  <img src="assets/benchmark_results.svg" alt="Benchmark results" width="70%">
</div>

## Getting started

### Installation

Karva is available as [`karva`](https://pypi.org/project/karva/) on PyPI.

Use karva directly with `uvx`:

```bash
uvx karva test
uvx karva version
```

Or install karva with `uv`, or `pip`:

```bash
# With uv.
uv tool install karva@latest
uv add --dev karva

# With pip.
pip install karva
```

### Usage

To run your tests, try any of the following:

```bash
# Run all tests.
karva test

# Run tests in a specific directory.
karva test tests/

# Run tests in a specific file.
karva test tests/test_example.py

# Run a specific test.
karva test tests/test_example(.py)::test_example
```

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for more information.
