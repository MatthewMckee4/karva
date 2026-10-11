//! Multiline inline snapshot mismatches exercise diff computation and rendering.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: 4,096 mismatches of 2,048-line snapshots ran in 2.561 s at 49.0 MiB
// peak RSS with a local release wheel; seven A/A pairs kept estimated variation below 5%.
pub(super) const CASES: usize = 4_096;
const LINES: usize = 2_048;

/// Changes a middle line while keeping a nonempty inline expectation and source unchanged.
pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    let expected = (0..LINES)
        .map(|line| format!("record {line:03}: αβγ/\\/\"quoted\""))
        .collect::<Vec<_>>()
        .join("\n");
    let actual = expected.replace(
        &format!("record {}:", LINES / 2),
        &format!("changed {}:", LINES / 2),
    );
    let mut source = format!("import karva\n\nEXPECTED = {expected:?}\nACTUAL = {actual:?}\n\n");
    for case in 0..CASES {
        writeln!(
            source,
            "def test_snapshot_diff_{case}():\n    karva.assert_snapshot(ACTUAL, inline=EXPECTED)\n"
        )?;
    }
    fs::write(tests.join("test_snapshot_diffs.py"), source)
        .context("Failed to write generated snapshot diff tests")
}
