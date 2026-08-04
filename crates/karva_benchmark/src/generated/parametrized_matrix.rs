//! Generated workload for large Cartesian parametrization expansion.

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: a 16³ matrix ran 4,096 tests in 0.88 s at 48.4 MiB peak RSS on
// arm64 macOS with a local debug wheel on 2026-08-04.
pub(super) const VALUES: usize = 16;

/// Generates a Cartesian parameter matrix, shaped on a small scale like:
///
/// ```python
/// @pytest.mark.parametrize("first", [0, 1])
/// @pytest.mark.parametrize("second", [0, 1])
/// def test_matrix(first, second): assert first + second >= 0
/// # Four test cases.
/// ```
pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    let values = (0..VALUES).collect::<Vec<_>>();
    fs::write(
        tests.join("test_parametrized_matrix.py"),
        format!(
            r#"import pytest

@pytest.mark.parametrize("first", {values:?})
@pytest.mark.parametrize("second", {values:?})
@pytest.mark.parametrize("third", {values:?})
def test_parametrized_matrix(first, second, third):
    assert first + second + third >= 0
"#,
        ),
    )
    .context("Failed to write generated parametrized tests")?;

    Ok(())
}
