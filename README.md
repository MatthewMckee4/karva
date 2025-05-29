# Karva (0.0.17)

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

By default, Karva will respect your `.gitignore` files when discovering tests in specified directories.

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
karva test tests/test.py
```

Provides the following output:

```bash
Discovered 3 tests
...
Failed tests:
tests.test::test_fail
File "/tests/test.py", line 6, in test_fail
  assert False, "This test should fail"
Error tests:
tests.test::test_error
File "/tests/test.py", line 10, in test_error
  raise ValueError("This is an error")
─────────────
Passed tests: 1
Failed tests: 1
Error tests: 1
```

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](CONTRIBUTING.md) for more information.
