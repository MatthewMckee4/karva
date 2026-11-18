//! Opt-in wall-time observations shared by workers and health reports.

use std::time::Duration;

use camino::Utf8PathBuf;
use ruff_source_file::SourceFile;
use ruff_text_size::TextRange;
use serde::{Deserialize, Serialize};

/// Exclusive execution phase; dependency setup is excluded from a fixture's setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TimingPhase {
    TestBody,
    FixtureSetup,
    FixtureTeardown,
}

/// One actual invocation, never multiplied by the number of fixture consumers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LifecycleTiming {
    pub name: String,

    pub phase: TimingPhase,

    pub path: Utf8PathBuf,

    /// One-based source line and Unicode column of the declaration.
    pub line: usize,

    pub column: usize,

    /// Zero-based UTF-8 declaration offsets.
    pub start: u32,

    pub end: u32,

    /// Wall-clock seconds in this invocation, including awaits and sleeps.
    pub duration_seconds: f64,

    /// Fixture lifetime; absent for a test body.
    pub scope: Option<String>,

    /// Direct dependency names, for relating exclusive setup costs.
    pub dependencies: Vec<String>,

    /// One-based attempt for a test body; fixture instances have independent lifetimes.
    pub attempt: Option<u32>,
}

impl LifecycleTiming {
    /// Captures source coordinates without reading or importing another file.
    pub fn new(
        name: String,
        source: &SourceFile,
        range: TextRange,
        phase: TimingPhase,
        duration: Duration,
    ) -> Self {
        let before = source
            .source_text()
            .get(..usize::from(range.start()))
            .unwrap_or_default();
        Self {
            name,
            phase,
            path: Utf8PathBuf::from(source.name()),
            line: before.bytes().filter(|byte| *byte == b'\n').count() + 1,
            column: before
                .rsplit('\n')
                .next()
                .unwrap_or_default()
                .chars()
                .count()
                + 1,
            start: range.start().to_u32(),
            end: range.end().to_u32(),
            duration_seconds: duration.as_secs_f64(),
            scope: None,
            dependencies: Vec::new(),
            attempt: None,
        }
    }
}
