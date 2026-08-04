//! Generated workload for dense fixture dependency resolution and execution.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: 128 fixtures across 1,024 tests ran in 2.91 s at 54.2 MiB peak RSS
// on arm64 macOS with a local debug wheel on 2026-08-04.
pub(super) const FIXTURES: usize = 128;
pub(super) const TESTS: usize = 1_024;

/// Generates fixtures requiring every earlier fixture, shaped on a small scale like:
///
/// ```python
/// @pytest.fixture
/// def fixture_0(): return 0
/// @pytest.fixture
/// def fixture_1(fixture_0): return 1
/// @pytest.fixture
/// def fixture_2(fixture_0, fixture_1): return 2
/// def test_dense_fixtures_0(fixture_2): assert fixture_2 == 2
/// ```
pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    let mut fixtures = String::from("import pytest\n\n");
    writeln!(
        fixtures,
        "@pytest.fixture\ndef fixture_0():\n    return 0\n"
    )?;
    for fixture in 1..FIXTURES {
        let dependencies = (0..fixture)
            .map(|dependency| format!("fixture_{dependency}"))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            fixtures,
            "@pytest.fixture\ndef fixture_{fixture}({dependencies}):\n    return {fixture}\n"
        )?;
    }
    fs::write(tests.join("conftest.py"), fixtures)
        .context("Failed to write generated dense fixtures")?;

    let mut source = String::new();
    for case in 0..TESTS {
        writeln!(
            source,
            "def test_dense_fixtures_{case}(fixture_{}):\n    assert fixture_{} == {}\n",
            FIXTURES - 1,
            FIXTURES - 1,
            FIXTURES - 1,
        )?;
    }
    fs::write(tests.join("test_dense_fixtures.py"), source)
        .context("Failed to write generated dense fixture tests")?;

    Ok(())
}
