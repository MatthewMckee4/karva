//! Cache artifact catalogue.
//!
//! The cache hierarchy has three levels — cache root, per-run directory, and
//! per-worker directory — and a small fixed set of files at each level. Each
//! file has a known on-disk format (pretty-printed JSON or plain text), so
//! pairing the filename with its serializer in one place means adding a new
//! artifact is a single-place change and read/write helpers can't drift.

use std::io::{ErrorKind, Write};

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;
use serde::Serialize;
use serde::de::DeserializeOwned;
use tempfile::NamedTempFile;

/// One of the well-known files in the cache directory hierarchy.
#[derive(Clone, Copy)]
pub enum CacheFile {
    /// Cache-root JSON, or legacy per-worker JSON: test wall-clock durations.
    Durations,
    /// Per-worker JSON: line-coverage data for sources tracked during the run.
    Coverage,
    /// Per-worker raw stderr captured by the controller.
    WorkerStderr,
    /// Cache-root JSON: list of last-run failed test names.
    LastFailed,
    /// Cache-root JSON: most recently generated random seed.
    RandomSeed,
}

impl CacheFile {
    /// Returns the on-disk filename for this artifact.
    pub const fn filename(self) -> &'static str {
        match self {
            Self::Durations => "durations.json",
            Self::Coverage => "coverage.json",
            Self::WorkerStderr => "stderr.log",
            Self::LastFailed => "last-failed.json",
            Self::RandomSeed => "random-seed.json",
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

/// Reads `dir/<file>` as JSON, or returns `Ok(None)` when the file does not exist.
pub fn read_json<T: DeserializeOwned>(dir: &Utf8Path, file: CacheFile) -> Result<Option<T>> {
    let path = file.path_in(dir);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    serde_json::from_str(&content)
        .with_context(|| format!("failed to parse cache artifact `{path}`"))
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_json_reports_artifact_path_on_parse_error() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let cache_dir =
            Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).expect("UTF-8 temp path");
        let path = CacheFile::LastFailed.path_in(&cache_dir);
        std::fs::write(&path, "not-json").expect("write malformed artifact");

        let error = read_json::<Vec<String>>(&cache_dir, CacheFile::LastFailed)
            .expect_err("parse should fail");

        assert!(
            error.to_string().contains(path.as_str()),
            "unexpected error: {error}"
        );
    }
}
