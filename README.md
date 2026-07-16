# Karva (0.0.1-alpha.7)

![PyPI - Version](https://img.shields.io/pypi/v/karva)

A Python test framework, written in Rust.

<div align="center">
  <img src="https://raw.githubusercontent.com/MatthewMckee4/karva/main/docs/assets/benchmark_results.svg" alt="Benchmark results" width="70%">
</div>

**We'd love for you to try Karva!** It's currently in alpha, and your feedback helps shape the project. [Get started](#getting-started) or join us on [Discord](https://discord.gg/XG95vNz4Zu).

## About Karva

Karva aims to be an efficient alternative to `pytest` and `unittest`.

Karva is intentionally narrower in scope than `pytest`. Not every pytest
feature encourages high quality tests, and we'd rather omit features than
ship ones that quietly make test suites worse. Karva draws on the Rust testing ecosystem,
where projects like [uv](https://github.com/astral-sh/uv) and
[ruff](https://github.com/astral-sh/ruff) show what a disciplined test suite
can look like. By keeping the surface area small, we hope to nudge
Python testing in the same direction.

[nextest](https://nexte.st) is the clearest example of what we're aiming for,
and meeting the bar it sets is an explicit goal of this project.

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

## Contributing

Contributions are welcome! See [CONTRIBUTING.md](https://github.com/MatthewMckee4/karva/blob/main/CONTRIBUTING.md) for more information.

## Support

Use [GitHub issues](https://github.com/MatthewMckee4/karva/issues) for bug reports and feature requests, or join the [Discord](https://discord.gg/XG95vNz4Zu) for discussion.

Please report security issues privately; see [SECURITY.md](https://github.com/MatthewMckee4/karva/blob/main/SECURITY.md).

## License

Karva is licensed under the [MIT License](https://github.com/MatthewMckee4/karva/blob/main/LICENSE).
