use std::collections::HashMap;
use std::fs;
use std::time::Duration;

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use karva_diagnostic::{TestResultStats, TestRunResult};
use ruff_db::diagnostic::{DisplayDiagnosticConfig, DisplayDiagnostics, FileResolver};

use crate::{
    DIAGNOSTICS_FILE, DISCOVER_DIAGNOSTICS_FILE, DURATIONS_FILE, RunHash, STATS_FILE, worker_folder,
};

/// Aggregated test results collected from all worker processes.
pub struct AggregatedResults {
    pub stats: TestResultStats,
    pub diagnostics: String,
    pub discovery_diagnostics: String,
}

/// Reads and writes test results in the cache directory for a specific run.
pub struct Cache {
    run_dir: Utf8PathBuf,
}

impl Cache {
    /// Constructs a cache handle for a specific run within the cache directory.
    pub fn new(cache_dir: &Utf8PathBuf, run_hash: &RunHash) -> Self {
        let run_dir = cache_dir.join(run_hash.to_string());
        Self { run_dir }
    }

    /// Reads and merges test results from all worker directories for this run.
    pub fn aggregate_results(&self) -> Result<AggregatedResults> {
        let mut test_stats = TestResultStats::default();
        let mut all_diagnostics = String::new();
        let mut all_discovery_diagnostics = String::new();

        if self.run_dir.exists() {
            let mut worker_dirs: Vec<Utf8PathBuf> = fs::read_dir(&self.run_dir)?
                .filter_map(|entry| {
                    let entry = entry.ok()?;
                    let path = Utf8PathBuf::try_from(entry.path()).ok()?;
                    if path.is_dir()
                        && path
                            .file_name()
                            .is_some_and(|name| name.starts_with("worker-"))
                    {
                        Some(path)
                    } else {
                        None
                    }
                })
                .collect();
            worker_dirs.sort();

            for worker_dir in &worker_dirs {
                read_worker_results(
                    worker_dir,
                    &mut test_stats,
                    &mut all_diagnostics,
                    &mut all_discovery_diagnostics,
                )?;
            }
        }

        Ok(AggregatedResults {
            stats: test_stats,
            diagnostics: all_diagnostics,
            discovery_diagnostics: all_discovery_diagnostics,
        })
    }

    /// Persists a test run result (stats, diagnostics, and durations) to disk.
    pub fn write_result(
        &self,
        worker_id: usize,
        result: &TestRunResult,
        resolver: &dyn FileResolver,
        config: &DisplayDiagnosticConfig,
    ) -> Result<()> {
        let worker_dir = self.run_dir.join(worker_folder(worker_id));
        fs::create_dir_all(&worker_dir)?;

        if !result.diagnostics().is_empty() {
            let output = DisplayDiagnostics::new(resolver, config, result.diagnostics());
            let path = worker_dir.join(DIAGNOSTICS_FILE);
            fs::write(path, output.to_string())?;
        }

        if !result.discovery_diagnostics().is_empty() {
            let output = DisplayDiagnostics::new(resolver, config, result.discovery_diagnostics());
            let path = worker_dir.join(DISCOVER_DIAGNOSTICS_FILE);
            fs::write(path, output.to_string())?;
        }

        let stats_path = worker_dir.join(STATS_FILE);
        let json = serde_json::to_string_pretty(result.stats())?;
        fs::write(&stats_path, json)?;

        let durations_path = worker_dir.join(DURATIONS_FILE);
        let json = serde_json::to_string_pretty(result.durations())?;
        fs::write(&durations_path, json)?;

        Ok(())
    }
}

/// Read results from a single worker directory.
fn read_worker_results(
    worker_dir: &Utf8Path,
    aggregated_stats: &mut TestResultStats,
    all_diagnostics: &mut String,
    all_discovery_diagnostics: &mut String,
) -> Result<()> {
    let stats_path = worker_dir.join(STATS_FILE);

    if stats_path.exists() {
        let content = fs::read_to_string(&stats_path)?;
        let stats = serde_json::from_str(&content)?;
        aggregated_stats.merge(&stats);
    }

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

/// Reads durations from the most recent test run.
///
/// Finds the most recent `run-{timestamp}` directory, then aggregates
/// all durations from all worker directories within it.
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

    run_dirs.sort_by_key(|hash| RunHash::from_existing(hash).sort_key());

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

        for (test_name, duration) in durations {
            aggregated_durations.insert(test_name, duration);
        }
    }

    Ok(aggregated_durations)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use camino::Utf8PathBuf;

    use super::*;

    fn create_cache_with_durations(
        dir: &std::path::Path,
        run_name: &str,
        worker_id: usize,
        durations: &HashMap<String, Duration>,
    ) {
        let worker_dir = dir.join(run_name).join(format!("worker-{worker_id}"));
        fs::create_dir_all(&worker_dir).unwrap();
        let json = serde_json::to_string(durations).unwrap();
        fs::write(worker_dir.join(DURATIONS_FILE), json).unwrap();
    }

    fn create_cache_with_stats(
        dir: &std::path::Path,
        run_name: &str,
        worker_id: usize,
        stats_json: &str,
    ) {
        let worker_dir = dir.join(run_name).join(format!("worker-{worker_id}"));
        fs::create_dir_all(&worker_dir).unwrap();
        fs::write(worker_dir.join(STATS_FILE), stats_json).unwrap();
    }

    #[test]
    fn read_recent_durations_returns_from_most_recent_run() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        let mut old_durations = HashMap::new();
        old_durations.insert("test_old".to_string(), Duration::from_millis(100));
        create_cache_with_durations(tmp.path(), "run-100", 0, &old_durations);

        let mut new_durations = HashMap::new();
        new_durations.insert("test_new".to_string(), Duration::from_millis(200));
        create_cache_with_durations(tmp.path(), "run-200", 0, &new_durations);

        let result = read_recent_durations(&cache_dir).unwrap();
        assert!(result.contains_key("test_new"));
        assert!(!result.contains_key("test_old"));
    }

    #[test]
    fn read_recent_durations_errors_when_no_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        let result = read_recent_durations(&cache_dir);
        assert!(result.is_err());
    }

    #[test]
    fn aggregate_results_merges_stats_from_multiple_workers() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        let run_hash = RunHash::from_existing("run-500");

        create_cache_with_stats(tmp.path(), "run-500", 0, r#"{"passed": 3, "failed": 1}"#);
        create_cache_with_stats(tmp.path(), "run-500", 1, r#"{"passed": 2, "skipped": 1}"#);

        let cache = Cache::new(&cache_dir, &run_hash);
        let results = cache.aggregate_results().unwrap();

        assert_eq!(results.stats.passed(), 5);
        assert_eq!(results.stats.failed(), 1);
        assert_eq!(results.stats.skipped(), 1);
    }

    #[test]
    fn aggregate_results_handles_missing_worker_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        let run_hash = RunHash::from_existing("run-600");
        let run_dir = tmp.path().join("run-600");
        fs::create_dir_all(&run_dir).unwrap();

        let cache = Cache::new(&cache_dir, &run_hash);
        let results = cache.aggregate_results().unwrap();

        assert_eq!(results.stats.total(), 0);
        assert!(results.diagnostics.is_empty());
    }
}
