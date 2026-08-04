//! Generated workload for large Cartesian parametrization expansion.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: 5 decorators with 7 values each expanded to 16,807 tests and ran in
// 0.92 s at 67.5 MiB peak RSS on arm64 macOS with a local debug wheel on
// 2026-08-04.
pub(super) const PARAMETRIZE_DECORATORS: usize = 5;
pub(super) const VALUES: usize = 7;

/// Generates a Cartesian parameter matrix, shaped on a small scale like:
///
/// ```python
/// @pytest.mark.parametrize("first", [0, 1])
/// @pytest.mark.parametrize("second", [0, 1])
/// @pytest.mark.parametrize("third", [0, 1])
/// def test_matrix(first, second, third): assert first + second + third >= 0
/// # Eight test cases.
/// ```
pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    let values = (0..VALUES).collect::<Vec<_>>();
    let mut parameters = Vec::with_capacity(PARAMETRIZE_DECORATORS);
    let mut source = String::from("import pytest\n\n");
    for parameter in 0..PARAMETRIZE_DECORATORS {
        let parameter = format!("parameter_{parameter}");
        writeln!(
            source,
            "@pytest.mark.parametrize({parameter:?}, {values:?})"
        )?;
        parameters.push(parameter);
    }
    writeln!(
        source,
        "def test_parametrized_matrix({}):\n    assert {} >= 0",
        parameters.join(", "),
        parameters.join(" + "),
    )?;
    fs::write(tests.join("test_parametrized_matrix.py"), source)
        .context("Failed to write generated parametrized tests")?;

    Ok(())
}
