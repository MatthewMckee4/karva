# Karva

![PyPI - Version](https://img.shields.io/pypi/v/karva)
[![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?logo=discord&logoColor=white)](https://discord.gg/XG95vNz4Zu)

A Python test framework, written in Rust. Parallel execution, fixtures,
async tests, snapshots, and coverage are built in.

Karva is in alpha. It runs many pytest tests, but does not support every pytest
feature. See the [non-goals](https://matthewmckee4.github.io/karva/usage/non-goals/)
before migrating a suite.

## Getting started

```bash
uv add --dev karva
uv run karva test
```

Run a directory or file:

```bash
uv run karva test tests/
uv run karva test tests/test_example.py
```

Karva respects `.gitignore` when discovering tests.
See the [tutorial](https://matthewmckee4.github.io/karva/get-started/tutorial/)
and [usage guide](https://matthewmckee4.github.io/karva/usage/) for more.

## Benchmarks

<div align="center">
  <img src="https://raw.githubusercontent.com/MatthewMckee4/karva/main/docs/assets/benchmark_results.svg" alt="Benchmark results" width="70%">
</div>

See the [benchmark project](https://github.com/MatthewMckee4/karva-benchmark-1)
for the workload and setup. Results depend on the suite and machine.

## Contributing and support

See [CONTRIBUTING.md](CONTRIBUTING.md) to contribute.
Use [GitHub issues](https://github.com/MatthewMckee4/karva/issues) for bugs and
feature requests, or [Discord](https://discord.gg/XG95vNz4Zu) for discussion.
Report security issues privately as described in [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE).
