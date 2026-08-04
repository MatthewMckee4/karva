//! Generated workload for deep fixture dependency resolution.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: 64 fixtures across 1,024 tests ran in 0.83 s at 47.5 MiB peak RSS
// on arm64 macOS with a local debug wheel on 2026-08-04.
pub(super) const DEPTH: usize = 64;
pub(super) const TESTS: usize = 1_024;

pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    let mut fixtures = String::from("import pytest\n\n");
    writeln!(
        fixtures,
        "@pytest.fixture\ndef fixture_0():\n    return 0\n"
    )?;
    for depth in 1..DEPTH {
        writeln!(
            fixtures,
            "@pytest.fixture\ndef fixture_{depth}(fixture_{}):\n    return fixture_{} + 1\n",
            depth - 1,
            depth - 1,
        )?;
    }
    fs::write(tests.join("conftest.py"), fixtures)
        .context("Failed to write generated nested fixtures")?;

    let mut source = String::new();
    for case in 0..TESTS {
        writeln!(
            source,
            "def test_nested_fixtures_{case}(fixture_{}):\n    assert fixture_{} == {}\n",
            DEPTH - 1,
            DEPTH - 1,
            DEPTH - 1,
        )?;
    }
    fs::write(tests.join("test_nested_fixtures.py"), source)
        .context("Failed to write generated nested fixture tests")?;

    Ok(())
}
