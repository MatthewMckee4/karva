use std::fs;

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use karva_diagnostic::TestResultStats;

use crate::{DIAGNOSTICS_FILE, DISCOVER_DIAGNOSTICS_FILE, RunHash, STATS_FILE, worker_folder};

pub struct AggregatedResults {
    pub stats: TestResultStats,
    pub diagnostics: String,
    pub discovery_diagnostics: String,
}

/// Reads and combines test results from the cache directory
///
/// Used by the main process to collect results from all worker processes.
pub struct CacheReader {
    run_dir: Utf8PathBuf,
    num_workers: usize,
}

impl CacheReader {
    pub fn new(cache_dir: &Utf8PathBuf, run_hash: &RunHash, num_workers: usize) -> Result<Self> {
        let run_dir = cache_dir.join(run_hash.to_string());

        Ok(Self {
            run_dir,
            num_workers,
        })
    }

    pub fn aggregate_results(&self) -> Result<AggregatedResults> {
        let mut test_stats = TestResultStats::default();
        let mut all_diagnostics = String::new();
        let mut all_discovery_diagnostics = String::new();

        for worker_id in 0..self.num_workers {
            let worker_dir = self.run_dir.join(worker_folder(worker_id));

            if !worker_dir.exists() {
                continue;
            }

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
    let stats_path = worker_dir.join(STATS_FILE);
    let content = fs::read_to_string(&stats_path)?;

    let stats = serde_json::from_str(&content)?;
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
