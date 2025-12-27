# Karva (0.0.1-alpha.1)

[![codecov](https://codecov.io/gh/MatthewMckee4/karva/graph/badge.svg?token=VELHBTE1L9)](https://codecov.io/gh/MatthewMckee4/karva)
![PyPI - Version](https://img.shields.io/pypi/v/karva)

> [!NOTE]
>
> Sadly, this will not be very well maintained and should not be used for anything important.
> I wanted to see if I could make a better `pytest`. But unless I completely diverge from the
> current architecture and basically start again, and try to make a better `pytest-xdist`, the
> performance of this will not be much better than `pytest`, let alone `pytest-xdist`.
>
> Thanks for taking an interest! Perhaps I will have a proper go at this in the future.

A Python test framework, written in Rust.

<div align="center">
  <img src="https://raw.githubusercontent.com/MatthewMckee4/karva/main/docs/assets/benchmark_results.svg" alt="Benchmark results" width="70%">
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

```py title="tests/test.py"
def test_pass():
    assert True


def test_fail():
    assert False, "This test should fail"


def test_error():
    raise ValueError("This is an error")
```

Running karva:

```bash
uv run karva test tests/
```

Provides the following output:

```text
test tests.test::test_pass ... ok
test tests.test::test_fail ... FAILED
test tests.test::test_error ... FAILED

diagnostics:

error[test-failure]: Test `test_fail` failed
 --> tests/test.py:5:5
  |
5 | def test_fail():
  |     ^^^^^^^^^
6 |     assert False, "This test should fail"
  |
info: Test failed here
 --> tests/test.py:6:5
  |
5 | def test_fail():
6 |     assert False, "This test should fail"
  |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
  |
info: Error message: This test should fail

error[test-failure]: Test `test_error` failed
  --> tests/test.py:9:5
   |
 9 | def test_error():
   |     ^^^^^^^^^^
10 |     raise ValueError("This is an error")
   |
info: Test failed here
  --> tests/test.py:10:5
   |
 9 | def test_error():
10 |     raise ValueError("This is an error")
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |
info: Error message: This is an error

test result: FAILED. 1 passed; 2 failed; 0 skipped; finished in 8ms
```

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](https://github.com/MatthewMckee4/karva/blob/main/CONTRIBUTING.md) for more information.

You can also join us on [Discord](https://discord.gg/XG95vNz4Zu)
