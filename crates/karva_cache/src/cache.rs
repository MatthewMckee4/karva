use std::collections::HashMap;
use std::fs;
use std::time::Duration;

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use karva_diagnostic::{TestResultStats, TestRunResult};
use ruff_db::diagnostic::{DisplayDiagnosticConfig, DisplayDiagnostics, FileResolver};

use crate::{
    DIAGNOSTICS_FILE, DISCOVER_DIAGNOSTICS_FILE, DURATIONS_FILE, FAIL_FAST_SIGNAL_FILE,
    FAILED_TESTS_FILE, LAST_FAILED_FILE, RunHash, STATS_FILE, worker_folder,
};

/// Aggregated test results collected from all worker processes.
pub struct AggregatedResults {
    pub stats: TestResultStats,
    pub diagnostics: String,
    pub discovery_diagnostics: String,
    pub failed_tests: Vec<String>,
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

    /// Writes a fail-fast signal file to indicate a worker encountered a test failure.
    pub fn write_fail_fast_signal(&self) -> Result<()> {
        fs::create_dir_all(&self.run_dir)?;
        let signal_path = self.run_dir.join(FAIL_FAST_SIGNAL_FILE);
        fs::write(signal_path, "")?;
        Ok(())
    }

    /// Checks whether any worker has written a fail-fast signal file.
    pub fn has_fail_fast_signal(&self) -> bool {
        self.run_dir.join(FAIL_FAST_SIGNAL_FILE).exists()
    }

    /// Reads and merges test results from all worker directories for this run.
    pub fn aggregate_results(&self) -> Result<AggregatedResults> {
        let mut test_stats = TestResultStats::default();
        let mut all_diagnostics = String::new();
        let mut all_discovery_diagnostics = String::new();
        let mut all_failed_tests = Vec::new();

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
                    &mut all_failed_tests,
                )?;
            }
        }

        Ok(AggregatedResults {
            stats: test_stats,
            diagnostics: all_diagnostics,
            discovery_diagnostics: all_discovery_diagnostics,
            failed_tests: all_failed_tests,
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

        if !result.failed_tests().is_empty() {
            let failed_tests: Vec<String> = result
                .failed_tests()
                .iter()
                .map(ToString::to_string)
                .collect();
            let failed_path = worker_dir.join(FAILED_TESTS_FILE);
            let json = serde_json::to_string_pretty(&failed_tests)?;
            fs::write(failed_path, json)?;
        }

        Ok(())
    }
}

/// Read results from a single worker directory.
fn read_worker_results(
    worker_dir: &Utf8Path,
    aggregated_stats: &mut TestResultStats,
    all_diagnostics: &mut String,
    all_discovery_diagnostics: &mut String,
    all_failed_tests: &mut Vec<String>,
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

    let failed_tests_path = worker_dir.join(FAILED_TESTS_FILE);
    if failed_tests_path.exists() {
        let content = fs::read_to_string(&failed_tests_path)?;
        let failed_tests: Vec<String> = serde_json::from_str(&content)?;
        all_failed_tests.extend(failed_tests);
    }

    Ok(())
}

/// Writes the list of failed tests to the cache directory root.
///
/// This overwrites any previous last-failed list.
pub fn write_last_failed(cache_dir: &Utf8Path, failed_tests: &[String]) -> Result<()> {
    fs::create_dir_all(cache_dir)?;
    let path = cache_dir.join(LAST_FAILED_FILE);
    let json = serde_json::to_string_pretty(failed_tests)?;
    fs::write(path, json)?;
    Ok(())
}

/// Reads the list of previously failed tests from the cache directory root.
///
/// Returns an empty list if the file does not exist.
pub fn read_last_failed(cache_dir: &Utf8Path) -> Result<Vec<String>> {
    let path = cache_dir.join(LAST_FAILED_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = fs::read_to_string(&path)?;
    let failed_tests: Vec<String> = serde_json::from_str(&content)?;
    Ok(failed_tests)
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

/// Result of a cache prune operation.
pub struct PruneResult {
    /// Names of the removed run directories.
    pub removed: Vec<String>,
}

/// Removes all but the most recent `run-*` directory from the cache.
pub fn prune_cache(cache_dir: &Utf8Path) -> Result<PruneResult> {
    let mut run_dirs = Vec::new();

    if let Ok(entries) = fs::read_dir(cache_dir) {
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
    }

    run_dirs.sort_by_key(|hash| RunHash::from_existing(hash).sort_key());

    let to_remove = run_dirs.len().saturating_sub(1);
    let mut removed = Vec::with_capacity(to_remove);

    for dir_name in run_dirs.iter().take(to_remove) {
        let path = cache_dir.join(dir_name);
        fs::remove_dir_all(&path)?;
        removed.push(dir_name.clone());
    }

    Ok(PruneResult { removed })
}

/// Removes the entire cache directory.
///
/// Returns `true` if the directory existed and was removed.
pub fn clean_cache(cache_dir: &Utf8Path) -> Result<bool> {
    if cache_dir.exists() {
        fs::remove_dir_all(cache_dir)?;
        Ok(true)
    } else {
        Ok(false)
    }
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
