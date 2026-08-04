//! Generated workload for flaky test re-execution and result tracking.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: 4,096 tests passing on their second attempt ran in 1.31 s at
// 53.9 MiB peak RSS on arm64 macOS with a local debug wheel on 2026-08-04.
pub(super) const CASES: usize = 4_096;

/// Generates tests that pass on retry, shaped on a small scale like:
///
/// ```python
/// def test_retry_0():
///     assert int(os.environ["KARVA_ATTEMPT"]) > 1
/// ```
pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    let mut source = String::from("import os\n\n");
    for case in 0..CASES {
        writeln!(
            source,
            "def test_retry_{case}():\n    assert int(os.environ[\"KARVA_ATTEMPT\"]) > 1\n"
        )?;
    }
    fs::write(tests.join("test_retries.py"), source)
        .context("Failed to write generated retry tests")?;

    Ok(())
}
