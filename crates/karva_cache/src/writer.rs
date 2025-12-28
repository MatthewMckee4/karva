use std::fs;

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use karva_diagnostic::TestRunResult;
use ruff_db::diagnostic::{DisplayDiagnosticConfig, DisplayDiagnostics, FileResolver};

use crate::{DIAGNOSTICS_FILE, DISCOVER_DIAGNOSTICS_FILE, STATS_FILE, models::RunHash};

/// Writes test results to the cache directory
///
/// Used by worker processes to persist test results incrementally.
pub struct CacheWriter {
    worker_dir: Utf8PathBuf,
}

impl CacheWriter {
    pub fn new(cache_dir: &Utf8PathBuf, run_hash: &RunHash, worker_id: usize) -> Result<Self> {
        let worker_dir = cache_dir
            .join(run_hash.to_string())
            .join(format!("worker-{worker_id}"));

        fs::create_dir_all(&worker_dir)?;

        Ok(Self { worker_dir })
    }

    pub fn write_result(
        &self,
        result: &TestRunResult,
        resolver: &dyn FileResolver,
        config: &DisplayDiagnosticConfig,
    ) -> Result<()> {
        if !result.diagnostics().is_empty() {
            let output = DisplayDiagnostics::new(resolver, config, result.diagnostics());
            let path = self.worker_dir.join(DIAGNOSTICS_FILE);
            fs::write(path, output.to_string())?;
        }

        if !result.discovery_diagnostics().is_empty() {
            let output = DisplayDiagnostics::new(resolver, config, result.discovery_diagnostics());
            let path = self.worker_dir.join(DISCOVER_DIAGNOSTICS_FILE);
            fs::write(path, output.to_string())?;
        }

        let stats_path = self.worker_dir.join(STATS_FILE);
        let json =
            serde_json::to_string_pretty(result.stats()).context("Failed to serialize stats")?;

        fs::write(&stats_path, json)
            .context(format!("Failed to write stats file: {stats_path}"))?;

        Ok(())
    }
}
