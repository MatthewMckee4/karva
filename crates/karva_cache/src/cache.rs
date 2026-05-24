use std::collections::HashMap;
use std::fs;
use std::time::Duration;

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use karva_diagnostic::{FlakyTest, TestResultStats, TestRunResult};
use ruff_db::diagnostic::{DisplayDiagnosticConfig, DisplayDiagnostics, FileResolver};

use crate::artifact::{CacheFile, read_json, read_text, write_json, write_json_if_nonempty};
use crate::{RUN_PREFIX, RunHash, WORKER_PREFIX, worker_folder};

/// Aggregated test results collected from all worker processes.
#[derive(Default)]
pub struct AggregatedResults {
    pub stats: TestResultStats,
    pub diagnostics: String,
    pub failed_tests: Vec<String>,
    pub flaky_tests: Vec<FlakyTest>,
    pub durations: HashMap<String, Duration>,
}

/// Reads and writes test results in the cache directory for a specific run.
#[derive(Clone)]
pub struct RunCache {
    run_dir: Utf8PathBuf,
}

impl RunCache {
    /// Constructs a cache handle for a specific run within the cache directory.
    pub fn new(cache_dir: &Utf8Path, run_hash: &RunHash) -> Self {
        let run_dir = cache_dir.join(run_hash.to_string());
        Self { run_dir }
    }

    /// Writes a fail-fast signal file to indicate a worker encountered a test failure.
    pub fn write_fail_fast_signal(&self) -> Result<()> {
        fs::create_dir_all(&self.run_dir)?;
        fs::write(CacheFile::FailFastSignal.path_in(&self.run_dir), "")?;
        Ok(())
    }

    /// Checks whether any worker has written a fail-fast signal file.
    pub fn has_fail_fast_signal(&self) -> bool {
        CacheFile::FailFastSignal.path_in(&self.run_dir).exists()
    }

    /// Reads and merges test results from all worker directories for this run.
    pub fn aggregate_results(&self) -> Result<AggregatedResults> {
        let mut results = AggregatedResults::default();

        for worker_dir in list_worker_dirs(&self.run_dir)? {
            read_worker_results(&worker_dir, &mut results)?;
        }

        Ok(results)
    }

    /// Path to the directory for a specific worker. Does not create it.
    pub fn worker_dir(&self, worker_id: usize) -> Utf8PathBuf {
        self.run_dir.join(worker_folder(worker_id))
    }

    /// Path to the per-worker coverage data file. The main process passes this
    /// to a worker via `--cov-data-file`; the worker writes the file when its
    /// coverage session ends.
    pub fn coverage_data_file(&self, worker_id: usize) -> Utf8PathBuf {
        CacheFile::Coverage.path_in(&self.worker_dir(worker_id))
    }

    /// Path to the per-worker progress file.
    ///
    /// The worker appends one byte to this file each time a test completes;
    /// the orchestrator polls the file lengths of every worker to drive a
    /// progress display. The append is atomic on POSIX, so readers can use
    /// the file length without locking.
    pub fn progress_file(&self, worker_id: usize) -> Utf8PathBuf {
        CacheFile::Progress.path_in(&self.worker_dir(worker_id))
    }

    /// Path to the per-worker output file.
    ///
    /// Workers append newline-delimited result lines to this file via
    /// `FileLineSink`. The orchestrator's drain reads new bytes since the
    /// last poll, splits on `\n`, and prints whole lines to its own stdout —
    /// keeping worker output from interleaving on a shared terminal.
    pub fn output_file(&self, worker_id: usize) -> Utf8PathBuf {
        CacheFile::Output.path_in(&self.worker_dir(worker_id))
    }

    /// Sum of completed-test counts across every worker for this run.
    ///
    /// Each worker's progress file is one byte per completed test, so the
    /// per-worker count is exactly the file length. Missing files (worker
    /// hasn't started yet, or no tests completed) contribute zero.
    pub fn completed_count(&self) -> u64 {
        let Ok(worker_dirs) = list_worker_dirs(&self.run_dir) else {
            return 0;
        };
        worker_dirs
            .iter()
            .map(|dir| {
                let path = CacheFile::Progress.path_in(dir);
                fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
            })
            .sum()
    }

    /// Returns paths to every per-worker coverage file that exists for this
    /// run, sorted by worker directory. Used to feed the coverage report.
    pub fn coverage_files(&self) -> Result<Vec<Utf8PathBuf>> {
        let mut files = Vec::new();
        for worker_dir in list_worker_dirs(&self.run_dir)? {
            let path = CacheFile::Coverage.path_in(&worker_dir);
            if path.exists() {
                files.push(path);
            }
        }
        Ok(files)
    }

    /// Persists a test run result (stats, diagnostics, durations, and failed tests) to disk.
    pub fn write_result(
        &self,
        worker_id: usize,
        result: &TestRunResult,
        resolver: &dyn FileResolver,
        config: &DisplayDiagnosticConfig,
    ) -> Result<()> {
        let worker_dir = self.worker_dir(worker_id);
        fs::create_dir_all(&worker_dir)?;

        write_diagnostics(&worker_dir, result, resolver, config)?;
        write_json(&worker_dir, CacheFile::Stats, result.stats())?;
        write_json(&worker_dir, CacheFile::Durations, result.durations())?;

        let failed_names: Vec<String> = result
            .failed_tests()
            .iter()
            .map(ToString::to_string)
            .collect();
        write_json_if_nonempty(&worker_dir, CacheFile::FailedTests, &failed_names)?;
        write_json_if_nonempty(&worker_dir, CacheFile::FlakyTests, result.flaky_tests())?;

        Ok(())
    }
}

/// Renders diagnostics into the worker directory.
///
/// Diagnostics use the ruff `DisplayDiagnostics` formatter rather than JSON,
/// so they don't share the [`write_json`] path; the file is skipped entirely
/// when there are no diagnostics.
fn write_diagnostics(
    worker_dir: &Utf8Path,
    result: &TestRunResult,
    resolver: &dyn FileResolver,
    config: &DisplayDiagnosticConfig,
) -> Result<()> {
    if result.diagnostics().is_empty() {
        return Ok(());
    }
    let output = DisplayDiagnostics::new(resolver, config, result.diagnostics());
    fs::write(
        CacheFile::Diagnostics.path_in(worker_dir),
        output.to_string(),
    )?;
    Ok(())
}

/// Reads results from a single worker directory into the accumulator.
fn read_worker_results(worker_dir: &Utf8Path, results: &mut AggregatedResults) -> Result<()> {
    if let Some(stats) = read_json::<TestResultStats>(worker_dir, CacheFile::Stats)? {
        results.stats.merge(&stats);
    }

    if let Some(content) = read_text(worker_dir, CacheFile::Diagnostics)? {
        results.diagnostics.push_str(&content);
    }

    if let Some(failed) = read_json::<Vec<String>>(worker_dir, CacheFile::FailedTests)? {
        results.failed_tests.extend(failed);
    }

    if let Some(flaky) = read_json::<Vec<FlakyTest>>(worker_dir, CacheFile::FlakyTests)? {
        results.flaky_tests.extend(flaky);
    }

    if let Some(durations) =
        read_json::<HashMap<String, Duration>>(worker_dir, CacheFile::Durations)?
    {
        results.durations.extend(durations);
    }

    Ok(())
}

/// Writes the list of failed tests to the cache directory root.
///
/// This overwrites any previous last-failed list.
pub fn write_last_failed(cache_dir: &Utf8Path, failed_tests: &[String]) -> Result<()> {
    fs::create_dir_all(cache_dir)?;
    write_json(cache_dir, CacheFile::LastFailed, &failed_tests)
}

/// Reads the list of previously failed tests from the cache directory root.
///
/// Returns an empty list if the file does not exist.
pub fn read_last_failed(cache_dir: &Utf8Path) -> Result<Vec<String>> {
    Ok(read_json::<Vec<String>>(cache_dir, CacheFile::LastFailed)?.unwrap_or_default())
}

/// Lists subdirectories of `parent` whose name starts with `prefix`.
///
/// Returns an empty vec if `parent` does not exist. Non-UTF-8 entries and
/// non-directory entries are silently skipped.
fn list_subdirs_with_prefix(parent: &Utf8Path, prefix: &str) -> Result<Vec<Utf8PathBuf>> {
    if !parent.exists() {
        return Ok(Vec::new());
    }

    let mut dirs = Vec::new();
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let Ok(path) = Utf8PathBuf::try_from(entry.path()) else {
            continue;
        };
        if path.is_dir()
            && path
                .file_name()
                .is_some_and(|name| name.starts_with(prefix))
        {
            dirs.push(path);
        }
    }
    Ok(dirs)
}

/// Returns sorted `worker-*` directories within a run directory.
fn list_worker_dirs(run_dir: &Utf8Path) -> Result<Vec<Utf8PathBuf>> {
    let mut dirs = list_subdirs_with_prefix(run_dir, WORKER_PREFIX)?;
    dirs.sort();
    Ok(dirs)
}

/// Returns `run-*` directory names sorted chronologically by their parsed timestamp.
fn collect_run_dirs(cache_dir: &Utf8Path) -> Result<Vec<String>> {
    let mut run_dirs: Vec<String> = list_subdirs_with_prefix(cache_dir, RUN_PREFIX)?
        .into_iter()
        .filter_map(|p| p.file_name().map(str::to_string))
        .collect();
    run_dirs.sort_by_key(|hash| RunHash::from_existing(hash).sort_key());
    Ok(run_dirs)
}

/// Reads durations from the most recent test run.
///
/// Finds the most recent `run-{timestamp}` directory, then aggregates
/// all durations from all worker directories within it.
pub fn read_recent_durations(cache_dir: &Utf8Path) -> Result<HashMap<String, Duration>> {
    let run_dirs = collect_run_dirs(cache_dir)?;
    let most_recent = run_dirs
        .last()
        .ok_or_else(|| anyhow::anyhow!("No run directories found"))?;
    let run_dir = cache_dir.join(most_recent);

    let mut aggregated_durations = HashMap::new();
    for worker_dir in list_worker_dirs(&run_dir)? {
        if let Some(durations) =
            read_json::<HashMap<String, Duration>>(&worker_dir, CacheFile::Durations)?
        {
            aggregated_durations.extend(durations);
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
    if !cache_dir.exists() {
        return Ok(PruneResult {
            removed: Vec::new(),
        });
    }

    let mut run_dirs = collect_run_dirs(cache_dir)?;

    let to_remove = run_dirs.len().saturating_sub(1);
    let mut removed = Vec::with_capacity(to_remove);

    for dir_name in run_dirs.drain(..to_remove) {
        let path = cache_dir.join(&dir_name);
        fs::remove_dir_all(&path)?;
        removed.push(dir_name);
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
    use insta::assert_debug_snapshot;

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
        fs::write(worker_dir.join(CacheFile::Durations.filename()), json).unwrap();
    }

    fn create_cache_with_stats(
        dir: &std::path::Path,
        run_name: &str,
        worker_id: usize,
        stats_json: &str,
    ) {
        let worker_dir = dir.join(run_name).join(format!("worker-{worker_id}"));
        fs::create_dir_all(&worker_dir).unwrap();
        fs::write(worker_dir.join(CacheFile::Stats.filename()), stats_json).unwrap();
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
        let run_name = run_hash.dir_name();

        create_cache_with_stats(tmp.path(), &run_name, 0, r#"{"passed": 3, "failed": 1}"#);
        create_cache_with_stats(tmp.path(), &run_name, 1, r#"{"passed": 2, "skipped": 1}"#);

        let cache = RunCache::new(&cache_dir, &run_hash);
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
        let run_dir = tmp.path().join(run_hash.dir_name());
        fs::create_dir_all(&run_dir).unwrap();

        let cache = RunCache::new(&cache_dir, &run_hash);
        let results = cache.aggregate_results().unwrap();

        assert_eq!(results.stats.total(), 0);
        assert!(results.diagnostics.is_empty());
    }

    #[test]
    fn write_last_failed_roundtrips_with_read() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        let failed = vec!["mod::test_a".to_string(), "mod::test_b".to_string()];
        write_last_failed(&cache_dir, &failed).unwrap();

        assert_debug_snapshot!(read_last_failed(&cache_dir).unwrap(), @r#"
        [
            "mod::test_a",
            "mod::test_b",
        ]
        "#);
    }

    #[test]
    fn read_last_failed_missing_file_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        let read = read_last_failed(&cache_dir).unwrap();
        assert!(read.is_empty());
    }

    #[test]
    fn write_last_failed_overwrites_previous_list() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        write_last_failed(&cache_dir, &["old".to_string()]).unwrap();
        write_last_failed(&cache_dir, &["new".to_string()]).unwrap();

        assert_debug_snapshot!(read_last_failed(&cache_dir).unwrap(), @r#"
        [
            "new",
        ]
        "#);
    }

    #[test]
    fn write_last_failed_creates_cache_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().join("nested").join("cache")).unwrap();
        assert!(!cache_dir.exists());

        write_last_failed(&cache_dir, &["x".to_string()]).unwrap();

        assert!(cache_dir.exists());
        assert_debug_snapshot!(read_last_failed(&cache_dir).unwrap(), @r#"
        [
            "x",
        ]
        "#);
    }

    #[test]
    fn read_last_failed_empty_json_list_parses() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        write_last_failed(&cache_dir, &[]).unwrap();
        assert!(read_last_failed(&cache_dir).unwrap().is_empty());
    }

    #[test]
    fn prune_cache_keeps_most_recent_run_only() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        for ts in ["run-100", "run-200", "run-300"] {
            fs::create_dir_all(tmp.path().join(ts)).unwrap();
        }

        let mut removed = prune_cache(&cache_dir).unwrap().removed;
        removed.sort();
        assert_debug_snapshot!(removed, @r#"
        [
            "run-100",
            "run-200",
        ]
        "#);
        assert!(cache_dir.join("run-300").exists());
        assert!(!cache_dir.join("run-100").exists());
        assert!(!cache_dir.join("run-200").exists());
    }

    #[test]
    fn prune_cache_handles_missing_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().join("nope")).unwrap();

        let result = prune_cache(&cache_dir).unwrap();
        assert!(result.removed.is_empty());
    }

    #[test]
    fn prune_cache_ignores_non_run_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        fs::create_dir_all(tmp.path().join("run-10")).unwrap();
        fs::create_dir_all(tmp.path().join("run-20")).unwrap();
        fs::create_dir_all(tmp.path().join("not-a-run")).unwrap();
        fs::write(tmp.path().join("last-failed.json"), "[]").unwrap();

        prune_cache(&cache_dir).unwrap();

        assert!(cache_dir.join("not-a-run").exists());
        assert!(cache_dir.join("last-failed.json").exists());
        assert!(cache_dir.join("run-20").exists());
        assert!(!cache_dir.join("run-10").exists());
    }

    #[test]
    fn prune_cache_keeps_newest_even_when_names_are_lexicographically_out_of_order() {
        // `run-9` lexicographically sorts AFTER `run-100` but numerically it is
        // older; pruning must use the numeric `sort_key` or it would delete the
        // newest run directory. This test guards against a regression to naive
        // string sorting.
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        fs::create_dir_all(tmp.path().join("run-9")).unwrap();
        fs::create_dir_all(tmp.path().join("run-100")).unwrap();

        prune_cache(&cache_dir).unwrap();

        assert!(cache_dir.join("run-100").exists());
        assert!(!cache_dir.join("run-9").exists());
    }

    #[test]
    fn clean_cache_removes_dir_and_returns_true() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        fs::create_dir_all(tmp.path().join("run-1")).unwrap();

        assert!(clean_cache(&cache_dir).unwrap());
        assert!(!cache_dir.exists());
    }

    #[test]
    fn clean_cache_missing_dir_returns_false() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().join("nope")).unwrap();
        assert!(!clean_cache(&cache_dir).unwrap());
    }

    #[test]
    fn aggregate_results_merges_failed_tests_and_durations_across_workers() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let run_hash = RunHash::from_existing("run-700");

        let run_dir = tmp.path().join(run_hash.dir_name());
        let worker0 = run_dir.join("worker-0");
        let worker1 = run_dir.join("worker-1");
        fs::create_dir_all(&worker0).unwrap();
        fs::create_dir_all(&worker1).unwrap();

        fs::write(
            worker0.join(CacheFile::FailedTests.filename()),
            r#"["mod::test_a"]"#,
        )
        .unwrap();
        fs::write(
            worker1.join(CacheFile::FailedTests.filename()),
            r#"["mod::test_b"]"#,
        )
        .unwrap();

        let mut d0 = HashMap::new();
        d0.insert("mod::test_a".to_string(), Duration::from_millis(10));
        let mut d1 = HashMap::new();
        d1.insert("mod::test_b".to_string(), Duration::from_millis(20));
        fs::write(
            worker0.join(CacheFile::Durations.filename()),
            serde_json::to_string(&d0).unwrap(),
        )
        .unwrap();
        fs::write(
            worker1.join(CacheFile::Durations.filename()),
            serde_json::to_string(&d1).unwrap(),
        )
        .unwrap();

        let cache = RunCache::new(&cache_dir, &run_hash);
        let results = cache.aggregate_results().unwrap();

        let mut failed = results.failed_tests.clone();
        failed.sort();
        assert_debug_snapshot!(failed, @r#"
        [
            "mod::test_a",
            "mod::test_b",
        ]
        "#);

        let mut durations: Vec<(String, Duration)> = results.durations.into_iter().collect();
        durations.sort();
        assert_debug_snapshot!(durations, @r#"
        [
            (
                "mod::test_a",
                10ms,
            ),
            (
                "mod::test_b",
                20ms,
            ),
        ]
        "#);
    }

    #[test]
    fn fail_fast_signal_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let run_hash = RunHash::from_existing("run-800");
        let cache = RunCache::new(&cache_dir, &run_hash);

        assert!(!cache.has_fail_fast_signal());
        cache.write_fail_fast_signal().unwrap();
        assert!(cache.has_fail_fast_signal());
    }
}
