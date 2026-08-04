//! Generated workload for repeated comparison of large inline snapshots.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: 4,096 tests each made 256 comparisons of a 2,048-line snapshot and
// ran in 1.48 s at 52.0 MiB peak RSS on arm64 macOS with a local debug wheel
// on 2026-08-04.
pub(super) const CASES: usize = 4_096;
pub(super) const LINES: usize = 2_048;
pub(super) const ASSERTIONS_PER_TEST: usize = 256;

/// Generates repeated large inline comparisons, shaped on a small scale like:
///
/// ```python
/// SNAPSHOT = "record 000\nrecord 001"
/// VALUE = SNAPSHOT.encode().decode()
/// def test_snapshot_0():
///     for _ in range(2):
///         karva.assert_snapshot(VALUE, inline=SNAPSHOT)
/// ```
pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    let snapshot = (0..LINES)
        .map(|line| format!("record {line:03}: αβγ/\\/\"quoted\""))
        .collect::<Vec<_>>()
        .join("\\n");
    let mut source =
        format!("import karva\n\nSNAPSHOT = {snapshot:?}\nVALUE = SNAPSHOT.encode().decode()\n\n");
    for case in 0..CASES {
        writeln!(
            source,
            "def test_snapshot_{case}():\n    for _ in range({ASSERTIONS_PER_TEST}):\n        karva.assert_snapshot(VALUE, inline=SNAPSHOT)\n"
        )?;
    }
    fs::write(tests.join("test_snapshots.py"), source)
        .context("Failed to write generated snapshot tests")?;

    Ok(())
}
