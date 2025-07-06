# Karva (0.1.3)

A Python test framework, written in Rust.

<div align="center">
  <img src="docs/assets/benchmark_results.svg" alt="Benchmark results" width="70%">
</div>

## About Karva

Karva aims to be an efficient alternative to `pytest` and `unittest`.

While we do not yet support all of pytest's features, we aim to gradually add support for pytest alternatives as we add features.

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

# Add karva to your project.
uv add --dev karva

# With pip.
pip install karva
```

### Usage

By default, Karva will respect your `.gitignore` files when discovering tests in specified directories.

To run your tests, try any of the following:

```bash
# Run all tests.
karva test

# Run tests in a specific directory.
karva test tests/

# Run tests in a specific file.
karva test tests/test_example.py
```

#### Example

Here is a small example usage

**tests/test.py**
```py
def test_pass():
    assert True


def test_fail():
    assert False, "This test should fail"


def test_error():
    raise ValueError("This is an error")
```

Running karva:

```bash
uv run karva test tests/test.py
```

Provides the following output:

```bash
fail[assertion-failed] in tests/test.py
 | File "tests/test.py", line 6, in test_fail
 |   assert False, "This test should fail"
error[value-error] in tests/test.py
 | File "/tests/test.py", line 10, in test_error
 |   raise ValueError("This is an error")

Passed tests: 1
Failed tests: 1
Errored tests: 1
```

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for more information.
