//! Many selected functions in one module expose quadratic name filtering.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: 8,000 plain functions completed in 0.85 s with a local debug wheel
// on arm64 macOS and Python 3.14 on 2026-09-26, before indexed selection.
pub(super) const TESTS: usize = 8_000;

pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    let mut source = String::new();
    for test in 0..TESTS {
        writeln!(source, "def test_{test}():\n    assert True\n")?;
    }
    fs::write(tests.join("test_functions.py"), source)
        .context("Failed to write generated function selection tests")
}
