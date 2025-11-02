# Karva - Codebase Guide

**Karva** is a high-performance Python test framework written in Rust (v0.1.7). It's a pytest-compatible alternative built for speed.

## Architecture

Test execution flow: **Discovery → Collection → Execution → Reporting**

### Workspace Structure
```
crates/
├── karva_core      # Core test engine (main logic here)
├── karva_cli       # CLI interface
├── karva_project   # Project management
├── karva           # Python bindings (PyO3)
├── karva_test      # Testing utilities
├── karva_benchmark # Benchmarking
└── karva_dev       # Dev utilities
```

## Where to Find Things

### `karva_core` - Core Components

All paths relative to `crates/karva_core/src/`:

#### Discovery (Finding tests)
- `discovery/discoverer.rs` - Main discovery logic
- `discovery/visitor.rs` - Python AST parsing
- `discovery/models/` - Package/Module/Function models

#### Collection (Organizing tests)
- `collection/collector.rs` - Test case collection
- `collection/models/case.rs` - Test case model

#### Runner (Executing tests)
- `runner/mod.rs` - **Main test runner and tests (1944 lines of tests here)**
- `runner/diagnostic.rs` - Test results tracking

#### Fixtures
- `extensions/fixtures/manager.rs` - Fixture lifecycle
- `extensions/fixtures/python.rs` - Python integration
- `extensions/fixtures/finalizer.rs` - Teardown handling
- `extensions/fixtures/builtins/` - Built-in fixtures (tmp_path, etc.)

#### Tags/Markers
- `extensions/tags/parametrize.rs` - `@parametrize` decorator
- `extensions/tags/skip.rs` - `@skip` decorator
- `extensions/tags/use_fixtures.rs` - `@use_fixtures` decorator

#### Diagnostics (Error reporting)
- `diagnostic/reporter.rs` - Reporter trait & implementations
- `diagnostic/diagnostic.rs` - Diagnostic messages
- `diagnostic/render.rs` - Output formatting

### Other Important Files
- `crates/karva_cli/src/lib.rs` - CLI logic (4112 lines)
- `crates/karva/` - Python bindings (exports `fixture`, `tags`, etc.)
- `python/karva/__init__.py` - Python package entry

## Running Tests

```bash
# Run all tests
cargo test

# Or use Just
just test

# Run specific test
cargo test test_name

# With logging
RUST_LOG=debug cargo test
```

**Main test file**: `crates/karva_core/src/runner/mod.rs` contains extensive integration tests.

## Building

```bash
# Build Python package
just build
# or: uv run --no-project maturin build

# Install locally
uv tool install karva@latest
```

## Key Technical Details

- **Python parsing**: Uses Ruff's parser (`ruff_python_ast`)
- **Python bindings**: PyO3 + Maturin
- **Python GIL**: Managed via `attach()` utility in `karva_core::utils`
- **Test discovery**: Uses `ignore` crate (respects `.gitignore`)
- **Fixture scopes**: function, module, package, session
- **pytest compat**: Supports both `pytest.fixture` and `karva.fixture`

## Adding Features

| Feature Type | Location |
|-------------|----------|
| New built-in fixture | `karva_core/src/extensions/fixtures/builtins/` |
| New tag/marker | `karva_core/src/extensions/tags/` |
| Discovery logic | `karva_core/src/discovery/visitor.rs` |
| New reporter | Implement `Reporter` trait in `diagnostic/reporter.rs` |
