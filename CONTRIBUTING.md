# Contributing to Karva

Thank you for considering contributing to Karva! We welcome contributions from everyone.

## Reporting Issues

If you encounter any issues or have suggestions for improvements, please [open an issue](https://github.com/MatthewMckee4/karva/issues/new).

## The Basics

For small changes (e.g., bug fixes), feel free to submit a PR.

For larger changes, consider creating an issue outlining your proposed change.

If you have suggestions on how we might improve the contributing documentation, let us know!

### Prerequisites

Karva is written in Rust. You can install the [Rust Toolchain](https://www.rust-lang.org/tools/install) to get started.

You can optionally install pre-commit hooks to automatically run the validation checks when making a commit:

```bash
uv tool install pre-commit
pre-commit install
```

### Development

```bash
cargo run test tests/test_add.py
```

### Running the tests

You must have `uv` installed to run the tests.

You must also first run `maturin build` to build the Python wheels.

```bash
cargo test
```

### Documentation

We use mkdocs to build the documentation.

```bash
mkdocs build
```
