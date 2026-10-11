//! Deterministic local and cross-module failures exercise traceback and diagnostic work.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: workload sizes and repetitions are measured in crates/karva_benchmark/measurements.md.
pub(super) const CASES: usize = 4_096;
const DEPTH: usize = 8;

/// Generates local assertion failures and a helper chain ending in an imported failure.
pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    fs::write(tests.join("__init__.py"), "")
        .context("Failed to write failure benchmark package")?;
    let mut helpers = String::from("def leaf():\n    raise ValueError(\"helper failure\")\n\n");
    for depth in 0..DEPTH {
        let target = if let Some(previous) = depth.checked_sub(1) {
            format!("helper_{previous}")
        } else {
            "leaf".to_string()
        };
        writeln!(helpers, "def helper_{depth}():\n    {target}()\n")?;
    }
    fs::write(tests.join("helpers.py"), helpers).context("Failed to write failure helpers")?;

    let mut source = format!("from .helpers import helper_{}\n\n", DEPTH - 1);
    for case in 0..CASES {
        let body = if case % 2 == 0 {
            "assert False, \"local failure\"".to_string()
        } else {
            format!("helper_{}()", DEPTH - 1)
        };
        writeln!(source, "def test_failure_{case}():\n    {body}\n")?;
    }
    fs::write(tests.join("test_failures.py"), source)
        .context("Failed to write generated failure tests")
}
