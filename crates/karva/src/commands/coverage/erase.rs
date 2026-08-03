//! Selective native coverage data deletion.

use std::io::ErrorKind;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use karva_coverage::native::NativeCoverage;

pub(super) fn erase(data_file: &Utf8Path) -> Result<()> {
    remove_if_present(data_file)?;
    let pending = data_file
        .parent()
        .unwrap_or_else(|| Utf8Path::new("."))
        .join("pending");
    for shard in recognized_shards(&pending)? {
        remove_if_present(&shard)?;
    }
    Ok(())
}

fn recognized_shards(directory: &Utf8Path) -> Result<Vec<Utf8PathBuf>> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read coverage shard directory `{directory}`"));
        }
    };
    let mut shards = Vec::new();
    for entry in entries {
        let entry = entry
            .with_context(|| format!("failed to read coverage shard directory `{directory}`"))?;
        let path = Utf8PathBuf::from_path_buf(entry.path()).map_err(|path| {
            anyhow::anyhow!(
                "coverage shard path contains non-Unicode characters: `{}`",
                path.display()
            )
        })?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect coverage shard `{path}`"))?;
        if file_type.is_dir() {
            shards.extend(recognized_shards(&path)?);
        } else if file_type.is_file() && NativeCoverage::read(&path).is_ok() {
            shards.push(path);
        }
    }
    shards.sort();
    Ok(shards)
}

fn remove_if_present(path: &Utf8Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => {
            Err(error).with_context(|| format!("failed to delete coverage data `{path}`"))
        }
    }
}
