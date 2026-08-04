//! Generated workload for discovery and import across many test modules.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: 8,192 modules with one test each ran in 1.36 s at 124.1 MiB peak
// RSS on arm64 macOS with a local debug wheel on 2026-08-04. Using 2,048
// modules with eight tests each exceeded the macOS process argument limit.
pub(super) const MODULES: usize = 8_192;
pub(super) const TESTS_PER_MODULE: usize = 1;

/// Generates many small modules, shaped on a small scale like:
///
/// ```text
/// tests/
///   test_0.py  # test_0_0, test_0_1
///   test_1.py  # test_1_0, test_1_1
/// ```
pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    for module in 0..MODULES {
        let mut source = String::new();
        for case in 0..TESTS_PER_MODULE {
            writeln!(source, "def test_{module}_{case}():\n    assert True\n")?;
        }
        fs::write(tests.join(format!("test_{module}.py")), source)
            .with_context(|| format!("Failed to write generated test module {module}"))?;
    }

    Ok(())
}
