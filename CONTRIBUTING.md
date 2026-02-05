# Contributing to Karva

Thank you for considering contributing to Karva! We welcome contributions from everyone.

Not only do we aim to make Karva a better tool for everyone, but we also aim to make the contributing process as smooth and enjoyable as possible.

So, if you come across any issues or have suggestions for improving development in the karva repo, please [open an issue](https://github.com/karva-dev/karva/issues/new).

## Reporting Issues

If you encounter any issues or have suggestions for improvements, please [open an issue](https://github.com/karva-dev/karva/issues/new).

## The Basics

For small changes (e.g., bug fixes), feel free to submit a PR.

For larger changes, consider creating an issue outlining your proposed change.

If you have suggestions on how we might improve the contributing documentation, let us know!

### Prerequisites

Karva is written in Rust. You can install the [Rust Toolchain](https://www.rust-lang.org/tools/install) to get started.

You can optionally install prek hooks to automatically run the validation checks when making a commit:

```bash
uv tool install prek
prek install
```

### Development

Note, you can use [just](https://github.com/casey/just) to run some useful commands.

To run the cli on a test file, run:

```bash
cargo run test tests/test_add.py
```

Annoyingly, you need a global python with pytest installed.

We have had many issues with local development using `uv` virtual environments with pytest installed, but this does not always work well.

If you want to run the tests, you need to build a wheel every time, so you need to run the following:

```bash
maturin build
cargo nextest run 
```

Or simply, with just, run:

```bash
just test
```

### Documentation

We use zensical to build the documentation.

```bash
uv run -s scripts/prepare_docs.py
uv run --isolated --only-group docs zensical build
```

## Release Process

Currently, everything is automated for releasing a new version of Karva.

First, install [seal](https://github.com/MatthewMckee4/seal), then bump the version with the following:


```bash
# Bump the alpha version
seal bump alpha

# Bump to a version
seal bump <version>
```

This will create a new branch and make a commit, so you just need to make a pull request.

## GitHub Actions

If you are updating github actions, ensure to run `pinact` to pin action versions.

```bash
pinact run
```
