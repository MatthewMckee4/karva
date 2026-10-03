//! Many modules sharing a substantial configuration expose repeated collection.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: 512 modules sharing 128 fixtures completed in 0.71 s with a local
// debug wheel on arm64 macOS and Python 3.14 on 2026-09-26, before ancestor reuse.
pub(super) const MODULES: usize = 512;
pub(super) const FIXTURES: usize = 128;

pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    let mut source = String::from("import pytest\n\n");
    for fixture in 0..FIXTURES {
        writeln!(
            source,
            "@pytest.fixture\ndef value_{fixture}():\n    return {fixture}\n"
        )?;
    }
    fs::write(tests.join("conftest.py"), source)
        .context("Failed to write shared benchmark configuration")?;
    for module in 0..MODULES {
        fs::write(
            tests.join(format!("test_{module}.py")),
            format!(
                "def test_value(value_{last}):\n    assert value_{last} == {last}\n",
                last = FIXTURES - 1
            ),
        )
        .with_context(|| format!("Failed to write shared-configuration test module {module}"))?;
    }
    Ok(())
}
