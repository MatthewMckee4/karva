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

To run the cli on a test file, run:

```bash
cargo run test tests/test_add.py
```

Annoyingly, you need a global python with pytest installed.

We have had many issues with local development using `uv` virtual environments with pytest installed, but this does not always work well.

So make sure you have python installed on your system, and pytest globally installed for that python version too.

To verify you have everything installed correctly, run:

```bash
python -c "import pytest;print('pytest installed')"
```

Then you can run the tests with:

```bash
cargo test
```

If you want to run the cli tests, you need to build a wheel file, so you need to run the following:

```bash
maturin build
cargo test -- --include-ignored
```

### Documentation

We use zensical to build the documentation.

```bash
uv run -s scripts/prepare_docs.py
uv run --isolated --only-group docs zensical build
```

## Release Process

Currently, everything is automated for releasing a new version of Karva.

Simply run the following command with your new version (eg `0.1.0`):

```bash
gcb bump-<new-version>
uv run --isolated --only-group release tbump <new-version> --no-tag --no-push
```

Once you have merged this branch, checkout main and pull the latest changes. Then run:

```bash
git tag --annotate --message v<new-version> v<new-version>
git push --atomic origin main v<new-version>
```

## GitHub Actions

If you are updating github actions, ensure to run `pinact` to pin action versions.

```bash
pinact run
```
