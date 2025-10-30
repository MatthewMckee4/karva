# Karva Diffs

The `karva_diffs` crate tracks diagnostic changes on real-world Python projects, similar to how mypy_primer works for mypy.

## Purpose

This crate helps track progress in pytest feature support by:
- Running Karva on real-world projects
- Capturing diagnostic output (test counts, errors, warnings)
- Automatically comparing diagnostics between main and PR branches
- Posting diff reports as PR comments

## How It Works

The diagnostic diff workflow runs automatically on every pull request:

1. **Runs diagnostics on main branch** - Establishes the baseline
2. **Runs diagnostics on PR branch** - Shows current state
3. **Generates a diff** - Compares the two results
4. **Posts a comment** - Shows improvements or regressions in the PR

## GitHub Actions Workflow

The `.github/workflows/diagnostic-diff.yml` workflow:
- Triggers on PR open, sync, or reopen
- Runs both sets of diagnostics
- Generates a markdown diff report
- Posts/updates a comment on the PR

Example comment output:
```markdown
# Diagnostic Diff Report

## Summary

| Project | Tests | Passed | Failed | Skipped | Errors | Warnings |
|---------|-------|--------|--------|---------|--------|----------|
| affect  | 50 (+5) | 45 (+5) | 5 | 0 | 5 | 2 (-1) |

## Detailed Changes

### affect

- **Passed tests:** 40 → 45 ✅
- **Warnings:** 3 → 2 ✅
```

## CLI Usage

The crate provides a `karva-diagnostics` binary for manual runs:

### Run diagnostics
```shell
# Run on all configured projects and output JSON
cargo run -p karva_diffs --bin karva-diagnostics -- run --output diagnostics.json

# Or just to stdout
cargo run -p karva_diffs --bin karva-diagnostics -- run
```

### Compare two reports
```shell
cargo run -p karva_diffs --bin karva-diagnostics -- diff \
  --base main-diagnostics.json \
  --head pr-diagnostics.json \
  --output diff.md
```

## Adding New Projects

To add a new project for tracking, edit `src/lib.rs` and add to the `get_test_projects()` function:

```rust
pub fn get_test_projects() -> Vec<RealWorldProject<'static>> {
    vec![
        RealWorldProject {
            name: "your-project",
            repository: "https://github.com/user/repo",
            commit: "abc123...",  // Pin to specific commit
            paths: vec![PathBuf::from("tests")],
            dependencies: vec!["pytest", "other-deps"],
            python_version: PythonVersion::PY313,
        },
        // ... more projects
    ]
}
```

## Interpreting Results

When reviewing a PR with diagnostic diffs:

- ✅ **Green checkmarks** - Improvements (more passing tests, fewer errors)
- ❌ **Red X marks** - Regressions (fewer passing tests, more errors)
- Numbers in parentheses - Show the change from main (e.g., `(+5)` means 5 more than main)

### What changes mean:

- **More passed tests** ✅ - New features working or bugs fixed
- **Fewer failed tests** ✅ - Bugs fixed or better compatibility
- **Fewer errors** ✅ - Improved error handling or detection
- **More failed tests** ❌ - Potential regression or new strict checks
- **More errors** ❌ - New issues introduced

## Development

The crate consists of:
- `src/lib.rs` - Core logic for running diagnostics and project registry
- `src/bin/karva-diagnostics.rs` - CLI tool for running and comparing diagnostics
- `.github/workflows/diagnostic-diff.yml` - GitHub Actions workflow

## Requirements

- Rust toolchain
- Python 3.13+
- `uv` package manager
- Network access (for cloning projects)

## Performance

Running diagnostics can take several minutes depending on:
- Number of projects configured
- Size of test suites
- Whether projects are cached (in `target/benchmark_cache/`)

The GitHub Actions workflow uses caching and concurrency controls to optimize performance.
