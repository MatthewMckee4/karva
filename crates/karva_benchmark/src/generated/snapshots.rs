//! Generated workload for repeated comparison of large inline snapshots.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: 128 snapshots of 128 lines ran in 0.72 s at 38.2 MiB peak RSS on
// arm64 macOS with a local debug wheel on 2026-08-04.
pub(super) const CASES: usize = 128;
pub(super) const LINES: usize = 128;

pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    let snapshot = (0..LINES)
        .map(|line| format!("record {line:03}: αβγ/\\/\"quoted\""))
        .collect::<Vec<_>>()
        .join("\\n");
    let mut source = format!("import karva\n\nSNAPSHOT = {snapshot:?}\n\n");
    for case in 0..CASES {
        writeln!(
            source,
            "def test_snapshot_{case}():\n    karva.assert_snapshot(SNAPSHOT, inline=SNAPSHOT)\n"
        )?;
    }
    fs::write(tests.join("test_snapshots.py"), source)
        .context("Failed to write generated snapshot tests")?;

    Ok(())
}
