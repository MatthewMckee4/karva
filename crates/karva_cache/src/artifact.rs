//! Cache artifact catalogue.
//!
//! The cache hierarchy has three levels — cache root, per-run directory, and
//! per-worker directory — and a small fixed set of files at each level. Each
//! file has a known on-disk format (pretty-printed JSON or plain text), so
//! pairing the filename with its serializer in one place means adding a new
//! artifact is a single-place change and read/write helpers can't drift.

use std::fs;

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// One of the well-known files in the cache directory hierarchy.
#[derive(Clone, Copy)]
pub enum CacheFile {
    /// Per-worker JSON: aggregated `TestResultStats`.
    Stats,
    /// Per-worker text: rendered diagnostics from discovery, collection, and execution.
    Diagnostics,
    /// Per-worker JSON: map of test id to wall-clock duration.
    Durations,
    /// Per-worker JSON: list of failed test names.
    FailedTests,
    /// Per-worker JSON: list of `FlakyTest` records.
    FlakyTests,
    /// Per-worker JSON: line-coverage data for sources tracked during the run.
    Coverage,
    /// Per-worker append-only file: one byte per completed test.
    ///
    /// Used by [`crate::RunCache::progress_file`] so the orchestrator can
    /// poll completion counts mid-run without coordinating with workers.
    Progress,
    /// Per-worker append-only file: newline-delimited preformatted result
    /// lines (PASS/FAIL/SKIP/SLOW/TRY). The orchestrator drains these files
    /// and prints whole lines to its stdout to prevent worker output from
    /// interleaving — multiple processes locking their own stdouts does not
    /// actually serialize writes. See [`crate::RunCache::output_file`].
    Output,
    /// Per-run empty sentinel marking that fail-fast was triggered.
    FailFastSignal,
    /// Cache-root JSON: list of last-run failed test names.
    LastFailed,
}

impl CacheFile {
    /// Returns the on-disk filename for this artifact.
    pub const fn filename(self) -> &'static str {
        match self {
            Self::Stats => "stats.json",
            Self::Diagnostics => "diagnostics.txt",
            Self::Durations => "durations.json",
            Self::FailedTests => "failed_tests.json",
            Self::FlakyTests => "flaky_tests.json",
            Self::Coverage => "coverage.json",
            Self::Progress => "progress",
            Self::Output => "output",
            Self::FailFastSignal => "fail-fast",
            Self::LastFailed => "last-failed.json",
        }
    }

    /// Joins this artifact's filename onto `dir`.
    pub fn path_in(self, dir: &Utf8Path) -> Utf8PathBuf {
        dir.join(self.filename())
    }
}

/// Pretty-prints `value` as JSON and writes it to `dir/<file>`.
pub fn write_json<T: Serialize>(dir: &Utf8Path, file: CacheFile, value: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(value)?;
    fs::write(file.path_in(dir), json)?;
    Ok(())
}

/// Like [`write_json`], but skips writing entirely when `items` is empty.
///
/// Used for artifacts where an empty list carries no information and the file
/// is treated as absent by readers.
pub fn write_json_if_nonempty<T: Serialize>(
    dir: &Utf8Path,
    file: CacheFile,
    items: &[T],
) -> Result<()> {
    if items.is_empty() {
        return Ok(());
    }
    write_json(dir, file, &items)
}

/// Reads `dir/<file>` as JSON, or returns `Ok(None)` when the file does not exist.
pub fn read_json<T: DeserializeOwned>(dir: &Utf8Path, file: CacheFile) -> Result<Option<T>> {
    let path = file.path_in(dir);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&path)?;
    Ok(Some(serde_json::from_str(&content)?))
}

/// Reads `dir/<file>` as raw text, or returns `Ok(None)` when the file does not exist.
pub fn read_text(dir: &Utf8Path, file: CacheFile) -> Result<Option<String>> {
    let path = file.path_in(dir);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(fs::read_to_string(&path)?))
}
