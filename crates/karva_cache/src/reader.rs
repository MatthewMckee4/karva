use std::collections::HashMap;
use std::fs;
use std::time::Duration;

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use karva_diagnostic::TestResultStats;

use crate::{
    DIAGNOSTICS_FILE, DISCOVER_DIAGNOSTICS_FILE, DURATIONS_FILE, RunHash, STATS_FILE, worker_folder,
};

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

/// Read durations from the most recent test run
///
/// This function finds the most recent run directory by sorting run-{timestamp:x}
/// directories, then aggregates all durations from all worker directories.
pub fn read_recent_durations(cache_dir: &Utf8PathBuf) -> Result<HashMap<String, Duration>> {
    let entries = fs::read_dir(cache_dir)?;

    let mut run_dirs = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) {
                if dir_name.starts_with("run-") {
                    run_dirs.push(dir_name.to_string());
                }
            }
        }
    }

    run_dirs.sort();

    let most_recent = run_dirs
        .last()
        .ok_or_else(|| anyhow::anyhow!("No run directories found"))?;

    let run_dir = cache_dir.join(most_recent);

    let mut aggregated_durations = HashMap::new();

    let worker_entries = fs::read_dir(&run_dir)?;

    for entry in worker_entries {
        let entry = entry?;
        let worker_path = Utf8PathBuf::try_from(entry.path())
            .map_err(|e| anyhow::anyhow!("Invalid UTF-8 path: {e}"))?;

        if !worker_path.is_dir() {
            continue;
        }

        let durations_path = worker_path.join(DURATIONS_FILE);
        if !durations_path.exists() {
            continue;
        }

        let content = fs::read_to_string(&durations_path)?;
        let durations: HashMap<String, Duration> = serde_json::from_str(&content)?;

        // Convert QualifiedTestName to String for easier lookup
        for (test_name, duration) in durations {
            aggregated_durations.insert(test_name, duration);
        }
    }

    Ok(aggregated_durations)
}
