//! Cache artifact catalogue.
//!
//! The cache hierarchy has three levels — cache root, per-run directory, and
//! per-worker directory — and a small fixed set of files at each level. Each
//! file has a known on-disk format (pretty-printed JSON or plain text), so
//! pairing the filename with its serializer in one place means adding a new
//! artifact is a single-place change and read/write helpers can't drift.

use std::fs;
use std::io::Write;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tempfile::NamedTempFile;

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
    /// Per-run empty sentinel marking that fail-fast was triggered.
    FailFastSignal,
    /// Cache-root JSON: list of last-run failed test names.
    LastFailed,
    /// Per-worker JSON: name + start time of the test currently executing,
    /// or empty/absent when the worker is between tests. Used by the
    /// orchestrator to render per-test `SIGINT` lines on Ctrl+C.
    CurrentTest,
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
            Self::FailFastSignal => "fail-fast",
            Self::LastFailed => "last-failed.json",
            Self::CurrentTest => "current_test.json",
        }
    }

    /// Joins this artifact's filename onto `dir`.
    pub fn path_in(self, dir: &Utf8Path) -> Utf8PathBuf {
        dir.join(self.filename())
    }
}

/// Pretty-prints `value` as JSON and writes it to `dir/<file>`.
pub fn write_json<T: Serialize>(dir: &Utf8Path, file: CacheFile, value: &T) -> Result<()> {
    let json = serde_json::to_vec_pretty(value)?;
    write_bytes(dir, file, &json)
}

/// Writes `content` to `dir/<file>`.
pub fn write_text(dir: &Utf8Path, file: CacheFile, content: impl AsRef<[u8]>) -> Result<()> {
    write_bytes(dir, file, content.as_ref())
}

fn write_bytes(dir: &Utf8Path, file: CacheFile, content: &[u8]) -> Result<()> {
    let path = file.path_in(dir);
    let parent = path
        .parent()
        .with_context(|| format!("cache artifact `{path}` has no parent directory"))?;

    let mut temp =
        NamedTempFile::new_in(parent).with_context(|| format!("failed to create `{path}`"))?;
    temp.write_all(content)
        .with_context(|| format!("failed to write `{path}`"))?;
    temp.flush()
        .with_context(|| format!("failed to flush `{path}`"))?;
    temp.persist(path.as_std_path())
        .map_err(|err| err.error)
        .with_context(|| format!("failed to replace `{path}`"))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_text_replaces_existing_artifact() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let cache_dir =
            Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).expect("UTF-8 temp path");

        write_text(&cache_dir, CacheFile::LastFailed, "old").expect("write old artifact");
        write_text(&cache_dir, CacheFile::LastFailed, "new").expect("replace artifact");

        let body = std::fs::read_to_string(CacheFile::LastFailed.path_in(&cache_dir))
            .expect("read artifact");
        assert_eq!(body, "new");
    }
}
