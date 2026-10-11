//! Python stdout and stderr capture, including output retained for failing tests.

use std::fmt::Write;

use anyhow::{Context, Result};
use camino::Utf8Path;
use fs_err as fs;

// Receipt: 4,096 tests writing eight 260-byte chunks per stream ran in 1.429 s at
// 60.5 MiB peak RSS with a local release wheel; seven A/A pairs kept variation below 5%.
pub(super) const CASES: usize = 4_096;
const WRITES: usize = 8;
const PADDING: usize = 245;

/// Emits fixed Unicode payloads to both streams; alternate tests fail after writing.
pub(super) fn generate(tests: &Utf8Path) -> Result<()> {
    let mut source = format!(
        "import sys\n\nSTDOUT = \"stdout αβγ \" + \"x\" * {PADDING} + \"\\n\"\nSTDERR = \"stderr αβγ \" + \"y\" * {PADDING} + \"\\n\"\n\n"
    );
    for case in 0..CASES {
        writeln!(
            source,
            "def test_output_{case}():\n    for _ in range({WRITES}):\n        sys.stdout.write(STDOUT)\n        sys.stderr.write(STDERR)"
        )?;
        if case % 2 == 1 {
            writeln!(source, "    assert False, \"captured failure\"")?;
        }
        writeln!(source)?;
    }
    fs::write(tests.join("test_output.py"), source)
        .context("Failed to write generated captured output tests")
}
