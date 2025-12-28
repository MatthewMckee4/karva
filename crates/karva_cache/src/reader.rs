use std::fs;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use karva_diagnostic::TestResultStats;

use crate::{DIAGNOSTICS_FILE, DISCOVER_DIAGNOSTICS_FILE, STATS_FILE, models::RunHash};

pub struct AggregatedResults {
    pub stats: TestResultStats,
    pub diagnostics: String,
    pub discovery_diagnostics: String,
}

/// Reads and aggregates test results from the cache directory
///
/// Used by the main process to collect results from all worker processes.
pub struct CacheReader {
    run_dir: Utf8PathBuf,
}

impl CacheReader {
    pub fn new(cache_dir: &Utf8PathBuf, run_hash: &RunHash) -> Result<Self> {
        let run_dir = cache_dir.join(run_hash.to_string());

        Ok(Self { run_dir })
    }

    /// Aggregate results from all workers
    ///
    /// # Arguments
    /// * `worker_count` - The number of workers that were spawned
    pub fn aggregate_results(&self, worker_count: usize) -> Result<AggregatedResults> {
        let mut test_stats = TestResultStats::default();
        let mut all_diagnostics = String::new();
        let mut all_discovery_diagnostics = String::new();

        for worker_id in 0..worker_count {
            let worker_dir = self.run_dir.join(format!("worker-{worker_id}"));

            // Skip if worker directory doesn't exist (worker may have crashed before writing)
            if !worker_dir.exists() {
                continue;
            }

            // Read all test results from this worker
            read_worker_results(
                &worker_dir,
                &mut test_stats,
                &mut all_diagnostics,
                &mut all_discovery_diagnostics,
            )?;
        }

        Ok(AggregatedResults {
            stats: test_stats,
            diagnostics: all_diagnostics,
            discovery_diagnostics: all_discovery_diagnostics,
        })
    }
}

/// Read results from a single worker directory
fn read_worker_results(
    worker_dir: &Utf8Path,
    aggregated_stats: &mut TestResultStats,
    all_diagnostics: &mut String,
    all_discovery_diagnostics: &mut String,
) -> Result<()> {
    tracing::debug!(worker_dir = %worker_dir, "Reading worker results");

    let stats_path = worker_dir.join(STATS_FILE);
    let content = fs::read_to_string(&stats_path)?;

    let stats = serde_json::from_str(&content).context("Failed to deserialize stats")?;

    aggregated_stats.merge(&stats);

    let diagnostics_path = worker_dir.join(DIAGNOSTICS_FILE);
    if diagnostics_path.exists() {
        let content = fs::read_to_string(&diagnostics_path)?;
        all_diagnostics.push_str(&content);
    }

    let discovery_diagnostics_path = worker_dir.join(DISCOVER_DIAGNOSTICS_FILE);
    if discovery_diagnostics_path.exists() {
        let content = fs::read_to_string(&discovery_diagnostics_path)?;
        all_discovery_diagnostics.push_str(&content);
    }

    Ok(())
}
