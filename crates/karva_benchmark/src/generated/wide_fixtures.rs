//! Generated workload for wide fixture dependency resolution and execution.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: 64 fixtures across 4,096 tests ran in 3.06 s at 138.7 MiB peak RSS
// on arm64 macOS with a local debug wheel on 2026-08-04.
pub(super) const FIXTURES: usize = 64;
pub(super) const TESTS: usize = 4_096;

/// Generates tests consuming every independent fixture, shaped on a small scale like:
///
/// ```python
/// @pytest.fixture
/// def fixture_0(): return 0
/// @pytest.fixture
/// def fixture_1(): return 1
/// def test_wide_fixtures_0(fixture_0, fixture_1):
///     assert fixture_0 + fixture_1 == 1
/// ```
pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    let mut fixtures = String::from("import pytest\n\n");
    for fixture in 0..FIXTURES {
        writeln!(
            fixtures,
            "@pytest.fixture\ndef fixture_{fixture}():\n    return {fixture}\n"
        )?;
    }
    fs::write(tests.join("conftest.py"), fixtures)
        .context("Failed to write generated wide fixtures")?;

    let arguments = (0..FIXTURES)
        .map(|fixture| format!("fixture_{fixture}"))
        .collect::<Vec<_>>();
    let expected = (0..FIXTURES).sum::<usize>();
    let mut source = String::new();
    for case in 0..TESTS {
        writeln!(
            source,
            "def test_wide_fixtures_{case}({}):\n    assert {} == {expected}\n",
            arguments.join(", "),
            arguments.join(" + "),
        )?;
    }
    fs::write(tests.join("test_wide_fixtures.py"), source)
        .context("Failed to write generated wide fixture tests")?;

    Ok(())
}
