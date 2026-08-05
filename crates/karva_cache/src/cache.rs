use std::collections::HashMap;
use std::io::ErrorKind;
use std::time::Duration;

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;
use karva_diagnostic::{
    DisplayDiagnosticConfig, FlakyTest, RenderedDiagnostic, TestCaseOutcome, TestCaseResult,
    TestResultKind, TestResultStats, TestRunResult, TestRunResultParts, render_diagnostic,
};
use karva_python_semantic::TestCacheKey;
use serde::{Deserialize, Serialize};

use crate::artifact::{CacheFile, read_json, read_text, write_json, write_text};
use crate::{RUN_PREFIX, RunHash, WORKER_PREFIX, worker_folder};

/// Aggregated test results collected from all worker processes.
#[derive(Default)]
pub struct AggregatedResults {
    /// Outcome counters merged across every worker.
    pub stats: TestResultStats,

    /// Collection and infrastructure diagnostics not owned by one test case.
    pub run_diagnostics: Vec<RenderedDiagnostic>,

    /// Base test names used to seed the next run's `--last-failed` selection.
    pub failed_tests: Vec<TestCacheKey>,

    /// Tests that passed only after one or more failed attempts.
    pub flaky_tests: Vec<FlakyTest>,

    /// Full result and retry history for every executed test case.
    pub test_cases: Vec<TestCaseResult>,

    /// Total duration keyed by qualified function or parameter case.
    pub durations: HashMap<TestCacheKey, Duration>,
}

/// Serializable result payload transferred from one worker to the controller.
#[derive(Default, Serialize, Deserialize)]
pub struct WorkerResults {
    stats: TestResultStats,
    run_diagnostics: Vec<RenderedDiagnostic>,
    #[serde(default)]
    failed_tests: Vec<TestCacheKey>,
    test_cases: Vec<TestCaseResult>,
    #[serde(default)]
    durations: HashMap<TestCacheKey, Duration>,
}

impl AggregatedResults {
    /// Whether every test and run-level diagnostic completed successfully.
    pub fn is_success(&self) -> bool {
        self.stats.is_success()
            && !self
                .run_diagnostics
                .iter()
                .any(RenderedDiagnostic::is_error)
    }

    /// Whether collection or worker infrastructure produced an error diagnostic.
    pub fn has_run_errors(&self) -> bool {
        self.run_diagnostics
            .iter()
            .any(RenderedDiagnostic::is_error)
    }

    /// Whether any test exhausted retries without passing.
    pub fn has_flaky_failures(&self) -> bool {
        self.test_cases.iter().any(TestCaseResult::is_flaky_failure)
    }

    /// Records a test interrupted by controller shutdown as a failed result.
    pub fn register_interrupted_test(&mut self, name: &str, duration: Duration) {
        let function_name = base_test_name(name);
        self.stats.add(TestResultKind::Failed);
        self.failed_tests.push(function_name.clone());
        self.test_cases.push(TestCaseResult::from_display_name(
            name,
            TestCaseOutcome::failed(RenderedDiagnostic::interrupted(name)),
            duration,
            None,
        ));
        self.durations.insert(function_name, duration);
    }

    pub fn merge_worker(&mut self, worker: WorkerResults) {
        for diagnostic in worker.run_diagnostics {
            if !self.run_diagnostics.contains(&diagnostic) {
                self.run_diagnostics.push(diagnostic);
            }
        }
        self.stats.merge(&worker.stats);
        if worker.failed_tests.is_empty() {
            self.failed_tests.extend(
                worker
                    .test_cases
                    .iter()
                    .filter(|case| case.outcome().is_non_success())
                    .map(|case| base_test_name(case.full_name())),
            );
        } else {
            self.failed_tests.extend(worker.failed_tests);
        }
        if worker.stats.flaky() > 0 {
            for case in &worker.test_cases {
                if let Some(retry) = case.retry()
                    && matches!(case.outcome(), TestCaseOutcome::Passed)
                {
                    self.flaky_tests.push(FlakyTest::from_display_name(
                        case.module_name(),
                        case.name(),
                        retry.attempts(),
                        retry.max_attempts(),
                        case.duration(),
                    ));
                }
            }
        }
        self.test_cases.extend(worker.test_cases);
        self.durations.extend(worker.durations);
    }
}

fn base_test_name(name: &str) -> TestCacheKey {
    let Some((module, function)) = name.rsplit_once("::") else {
        return TestCacheKey::function_name(name);
    };
    let function = function
        .split_once('(')
        .or_else(|| function.split_once('['))
        .map_or(function, |(base, _)| base);
    TestCacheKey::function_name(&format!("{module}::{function}"))
}

/// Reads and writes test results in the cache directory for a specific run.
pub struct RunCache {
    /// Run-scoped directory containing every worker's artifacts.
    run_dir: Utf8PathBuf,
}

impl RunCache {
    /// Constructs a cache handle for a specific run within the cache directory.
    pub fn new(cache_dir: &Utf8Path, run_hash: &RunHash) -> Self {
        let run_dir = cache_dir.join(run_hash.to_string());
        Self { run_dir }
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
    fn worker_dir(&self, worker_id: usize) -> Utf8PathBuf {
        self.run_dir.join(worker_folder(worker_id))
    }

    /// Path to the per-worker coverage data file. The main process passes this
    /// to a worker via `--cov-data-file`; the worker writes the file when its
    /// coverage session ends.
    pub fn coverage_data_file(&self, worker_id: usize) -> Utf8PathBuf {
        CacheFile::Coverage.path_in(&self.worker_dir(worker_id))
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

    /// Persists a test run result to disk.
    pub fn write_result(
        &self,
        worker_id: usize,
        result: TestRunResult,
        cwd: &Utf8Path,
        config: &DisplayDiagnosticConfig,
    ) -> Result<()> {
        let worker_dir = self.worker_dir(worker_id);
        fs::create_dir_all(&worker_dir)?;

        let worker_results = render_worker_results(result, cwd, config)?;

        write_json(&worker_dir, CacheFile::Durations, &worker_results.durations)?;
        write_text(
            &worker_dir,
            CacheFile::Results,
            serde_json::to_vec(&worker_results)?,
        )?;
        Ok(())
    }
}

/// Renders worker-owned diagnostics and converts semantic names for transport.
pub fn render_worker_results(
    result: TestRunResult,
    cwd: &Utf8Path,
    config: &DisplayDiagnosticConfig,
) -> Result<WorkerResults> {
    let TestRunResultParts {
        run_diagnostics,
        stats,
        durations,
        failed_tests,
        test_cases,
    } = result.into_parts();
    let run_diagnostics = run_diagnostics
        .iter()
        .map(|diagnostic| render_diagnostic(diagnostic, cwd, *config))
        .collect::<Vec<_>>();
    let test_cases = test_cases
        .into_iter()
        .map(|case| {
            case.try_map_diagnostic(|diagnostic| {
                Ok::<_, anyhow::Error>(render_diagnostic(diagnostic, cwd, *config))
            })
        })
        .collect::<Result<Vec<TestCaseResult>>>()?;

    let mut failed_tests = failed_tests.into_iter().collect::<Vec<_>>();
    failed_tests.sort();

    Ok(WorkerResults {
        stats,
        run_diagnostics,
        failed_tests,
        test_cases,
        durations,
    })
}

/// Reads results from a single worker directory into the accumulator.
fn read_worker_results(worker_dir: &Utf8Path, results: &mut AggregatedResults) -> Result<()> {
    let Some(worker_results) = read_json::<WorkerResults>(worker_dir, CacheFile::Results)? else {
        return Ok(());
    };
    let has_embedded_durations = !worker_results.durations.is_empty();
    results.merge_worker(worker_results);

    if !has_embedded_durations
        && let Some(durations) =
            read_json::<HashMap<TestCacheKey, Duration>>(worker_dir, CacheFile::Durations)?
    {
        results.durations.extend(durations);
    }

    Ok(())
}

/// Writes the list of failed tests to the cache directory root.
///
/// This overwrites any previous last-failed list.
pub fn write_last_failed(cache_dir: &Utf8Path, failed_tests: &[TestCacheKey]) -> Result<()> {
    fs::create_dir_all(cache_dir)?;
    write_json(cache_dir, CacheFile::LastFailed, &failed_tests)
}

/// Reads the list of previously failed tests from the cache directory root.
///
/// Returns an empty list if the file does not exist.
pub fn read_last_failed(cache_dir: &Utf8Path) -> Result<Vec<TestCacheKey>> {
    Ok(read_json::<Vec<TestCacheKey>>(cache_dir, CacheFile::LastFailed)?.unwrap_or_default())
}

/// Persists the most recently generated random seed.
pub fn write_random_seed(cache_dir: &Utf8Path, seed: u64) -> Result<()> {
    fs::create_dir_all(cache_dir)?;
    write_json(cache_dir, CacheFile::RandomSeed, &seed)
}

/// Reads the most recently generated random seed.
pub fn read_random_seed(cache_dir: &Utf8Path) -> Result<Option<u64>> {
    read_json(cache_dir, CacheFile::RandomSeed)
}

/// Lists subdirectories of `parent` whose name starts with `prefix`.
///
/// Returns an empty vec if `parent` does not exist. Non-UTF-8 entries and
/// non-directory entries are silently skipped.
fn list_subdirs_with_prefix(parent: &Utf8Path, prefix: &str) -> Result<Vec<Utf8PathBuf>> {
    let mut dirs = Vec::new();
    let entries = match fs::read_dir(parent) {
        Ok(entries) => entries,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    for entry in entries {
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
    dirs.sort_by(|a, b| {
        worker_dir_sort_key(a)
            .cmp(&worker_dir_sort_key(b))
            .then_with(|| a.cmp(b))
    });
    Ok(dirs)
}

fn worker_dir_sort_key(path: &Utf8Path) -> (bool, usize) {
    worker_dir_id(path).map_or((true, usize::MAX), |id| (false, id))
}

fn worker_dir_id(path: &Utf8Path) -> Option<usize> {
    path.file_name()?.strip_prefix(WORKER_PREFIX)?.parse().ok()
}

/// Returns `run-*` directory names sorted chronologically by their parsed timestamp.
fn collect_run_dirs(cache_dir: &Utf8Path) -> Result<Vec<String>> {
    let mut run_dirs: Vec<String> = list_subdirs_with_prefix(cache_dir, RUN_PREFIX)?
        .into_iter()
        .filter_map(|p| p.file_name().map(str::to_string))
        .collect();
    run_dirs.sort_by(|a, b| {
        RunHash::from_existing(a)
            .sort_key()
            .cmp(&RunHash::from_existing(b).sort_key())
            .then_with(|| a.cmp(b))
    });
    Ok(run_dirs)
}

/// Reads durations from the most recent test run.
///
/// Finds the most recent `run-{timestamp}` directory, then aggregates
/// all durations from all worker directories within it.
pub fn read_recent_durations(cache_dir: &Utf8Path) -> Result<HashMap<TestCacheKey, Duration>> {
    let run_dirs = collect_run_dirs(cache_dir)?;
    let Some(most_recent) = run_dirs.last() else {
        return Ok(HashMap::new());
    };
    let run_dir = cache_dir.join(most_recent);

    let mut aggregated_durations = HashMap::new();
    for worker_dir in list_worker_dirs(&run_dir)? {
        if let Some(durations) =
            read_json::<HashMap<TestCacheKey, Duration>>(&worker_dir, CacheFile::Durations)?
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
    match fs::remove_dir_all(cache_dir) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use camino::Utf8PathBuf;
    use insta::assert_debug_snapshot;
    use karva_diagnostic::CapturedTestOutput;
    use karva_diagnostic::Severity;

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
        let stats: TestResultStats =
            serde_json::from_str(stats_json).expect("deserialize test stats");
        let mut test_cases = Vec::new();
        for index in 0..stats.passed() {
            test_cases.push(TestCaseResult::from_display_name(
                &format!("mod::test_passed_{index}"),
                TestCaseOutcome::Passed,
                Duration::ZERO,
                None,
            ));
        }
        for index in 0..stats.failed() {
            test_cases.push(failed_case(&format!("mod::test_failed_{index}")));
        }
        for index in 0..stats.skipped() {
            test_cases.push(TestCaseResult::from_display_name(
                &format!("mod::test_skipped_{index}"),
                TestCaseOutcome::Skipped { reason: None },
                Duration::ZERO,
                None,
            ));
        }
        write_worker_results(
            &worker_dir,
            &WorkerResults {
                stats,
                run_diagnostics: Vec::new(),
                failed_tests: Vec::new(),
                test_cases,
                durations: HashMap::new(),
            },
        );
    }

    fn failed_case(name: &str) -> TestCaseResult {
        TestCaseResult::from_display_name(
            name,
            TestCaseOutcome::failed(RenderedDiagnostic::new(
                "test-failure",
                Severity::Error,
                "failed",
                "error[test-failure]: failed\n",
            )),
            Duration::ZERO,
            None,
        )
    }

    fn write_worker_results(worker_dir: &std::path::Path, results: &WorkerResults) {
        fs::write(
            worker_dir.join(CacheFile::Results.filename()),
            serde_json::to_string(results).expect("serialize worker results"),
        )
        .expect("write worker results");
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
    fn read_recent_durations_returns_empty_when_no_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        let result = read_recent_durations(&cache_dir).unwrap();
        assert!(result.is_empty());
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
        assert!(results.run_diagnostics.is_empty());
    }

    #[test]
    fn aggregate_results_ignores_incomplete_worker() {
        let tmp = tempfile::tempdir().expect("create temporary directory");
        let cache_dir =
            Utf8PathBuf::try_from(tmp.path().to_path_buf()).expect("temporary path is UTF-8");
        let run_hash = RunHash::from_existing("run-610");
        let worker_dir = tmp.path().join(run_hash.dir_name()).join("worker-0");
        fs::create_dir_all(&worker_dir).expect("create worker directory");

        let durations = HashMap::from([(
            "mod::test_uncommitted".to_string(),
            Duration::from_millis(10),
        )]);
        fs::write(
            worker_dir.join(CacheFile::Durations.filename()),
            serde_json::to_string(&durations).expect("serialize durations"),
        )
        .expect("write durations");

        let results = RunCache::new(&cache_dir, &run_hash)
            .aggregate_results()
            .expect("aggregate results");

        assert!(results.durations.is_empty());
    }

    #[test]
    fn worker_results_reject_missing_fields() {
        assert!(serde_json::from_str::<WorkerResults>("{}").is_err());
    }

    #[test]
    fn list_worker_dirs_sorts_by_numeric_worker_id() {
        let tmp = tempfile::tempdir().unwrap();
        let run_dir = Utf8PathBuf::try_from(tmp.path().join("run-1")).unwrap();

        for name in ["worker-10", "worker-2", "worker-old", "worker-1"] {
            fs::create_dir_all(run_dir.join(name)).unwrap();
        }

        let names: Vec<_> = list_worker_dirs(&run_dir)
            .unwrap()
            .into_iter()
            .filter_map(|path| path.file_name().map(str::to_string))
            .collect();

        assert_debug_snapshot!(names, @r#"
        [
            "worker-1",
            "worker-2",
            "worker-10",
            "worker-old",
        ]
        "#);
    }

    #[test]
    fn write_last_failed_roundtrips_with_read() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        let failed = vec![
            TestCacheKey::function_name("mod::test_a"),
            TestCacheKey::function_name("mod::test_b"),
        ];
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
    fn random_seed_roundtrips() {
        let tmp = tempfile::tempdir().expect("create temp directory");
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).expect("UTF-8 temp path");

        write_random_seed(&cache_dir, 170_938).expect("write random seed");

        assert_eq!(
            read_random_seed(&cache_dir).expect("read random seed"),
            Some(170_938)
        );
    }

    #[test]
    fn write_last_failed_overwrites_previous_list() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        write_last_failed(&cache_dir, &[TestCacheKey::function_name("old")]).unwrap();
        write_last_failed(&cache_dir, &[TestCacheKey::function_name("new")]).unwrap();

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

        write_last_failed(&cache_dir, &[TestCacheKey::function_name("x")]).unwrap();

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
    fn prune_cache_reports_cache_dir_read_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().join("cache")).unwrap();
        fs::write(&cache_dir, "").unwrap();

        let Err(error) = prune_cache(&cache_dir) else {
            panic!("file cache path should fail");
        };

        assert!(
            error.to_string().contains(cache_dir.as_str()),
            "unexpected error: {error}"
        );
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
    fn collect_run_dirs_breaks_equal_timestamp_ties_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        for name in [
            "run-100-00000000-0000-4000-8000-000000000002",
            "run-100-00000000-0000-4000-8000-000000000001",
            "run-90-00000000-0000-4000-8000-000000000099",
        ] {
            fs::create_dir_all(tmp.path().join(name)).unwrap();
        }

        assert_debug_snapshot!(collect_run_dirs(&cache_dir).unwrap(), @r#"
        [
            "run-90-00000000-0000-4000-8000-000000000099",
            "run-100-00000000-0000-4000-8000-000000000001",
            "run-100-00000000-0000-4000-8000-000000000002",
        ]
        "#);
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
        let mut worker0_stats = TestResultStats::default();
        worker0_stats.add(TestResultKind::Failed);
        let mut worker1_stats = TestResultStats::default();
        worker1_stats.add(TestResultKind::Failed);

        write_worker_results(
            &worker0,
            &WorkerResults {
                stats: worker0_stats,
                test_cases: vec![failed_case("mod::test_a")],
                ..WorkerResults::default()
            },
        );
        write_worker_results(
            &worker1,
            &WorkerResults {
                stats: worker1_stats,
                test_cases: vec![failed_case("mod::test_b")],
                ..WorkerResults::default()
            },
        );

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

        let mut durations: Vec<(TestCacheKey, Duration)> = results.durations.into_iter().collect();
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
    fn aggregate_results_merges_test_output_across_workers() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        let run_hash = RunHash::from_existing("run-710");

        let run_dir = tmp.path().join(run_hash.dir_name());
        let worker0 = run_dir.join("worker-0");
        let worker1 = run_dir.join("worker-1");
        fs::create_dir_all(&worker0).unwrap();
        fs::create_dir_all(&worker1).unwrap();
        let mut worker0_stats = TestResultStats::default();
        worker0_stats.add(TestResultKind::Passed);
        let mut worker1_stats = TestResultStats::default();
        worker1_stats.add(TestResultKind::Passed);

        let case0: TestCaseResult = TestCaseResult::from_display_name(
            "mod::test_a",
            TestCaseOutcome::Passed,
            Duration::ZERO,
            Some(CapturedTestOutput::new(
                "worker 0 stdout\n".to_string(),
                String::new(),
            )),
        );
        let case1: TestCaseResult = TestCaseResult::from_display_name(
            "mod::test_b",
            TestCaseOutcome::Passed,
            Duration::ZERO,
            Some(CapturedTestOutput::new(
                String::new(),
                "worker 1 stderr\n".to_string(),
            )),
        );

        write_worker_results(
            &worker0,
            &WorkerResults {
                stats: worker0_stats,
                test_cases: vec![case0],
                ..WorkerResults::default()
            },
        );
        write_worker_results(
            &worker1,
            &WorkerResults {
                stats: worker1_stats,
                test_cases: vec![case1],
                ..WorkerResults::default()
            },
        );

        let cache = RunCache::new(&cache_dir, &run_hash);
        let mut cases = cache
            .aggregate_results()
            .expect("aggregate results")
            .test_cases;
        cases.sort_by(|a, b| a.full_name().cmp(b.full_name()));

        assert_debug_snapshot!(cases, @r#"
        [
            TestCaseResult {
                module_name: "mod",
                name: "test_a",
                full_name: "mod::test_a",
                outcome: Passed,
                duration: 0ns,
                retry: None,
                captured_output: Some(
                    CapturedTestOutput {
                        stdout: "worker 0 stdout\n",
                        stderr: "",
                    },
                ),
                attempts: [],
            },
            TestCaseResult {
                module_name: "mod",
                name: "test_b",
                full_name: "mod::test_b",
                outcome: Passed,
                duration: 0ns,
                retry: None,
                captured_output: Some(
                    CapturedTestOutput {
                        stdout: "",
                        stderr: "worker 1 stderr\n",
                    },
                ),
                attempts: [],
            },
        ]
        "#);
    }

    #[test]
    fn interrupted_tests_count_as_failures() {
        let mut results = AggregatedResults::default();

        results.register_interrupted_test("mod::test_slow", Duration::from_millis(42));

        assert_eq!(results.stats.failed(), 1);
        assert_debug_snapshot!(results.failed_tests, @r#"
        [
            "mod::test_slow",
        ]
        "#);
        assert_debug_snapshot!(results.durations, @r#"
        {
            "mod::test_slow": 42ms,
        }
        "#);
    }

    #[test]
    fn interrupted_parametrized_tests_store_base_function_name_for_reruns() {
        let mut results = AggregatedResults::default();

        results.register_interrupted_test("mod::test_slow(value=1)", Duration::from_millis(42));
        results.register_interrupted_test("mod::test_legacy[value]", Duration::from_millis(24));

        assert_debug_snapshot!(results.failed_tests, @r#"
        [
            "mod::test_slow",
            "mod::test_legacy",
        ]
        "#);
        let mut durations: Vec<_> = results.durations.iter().collect();
        durations.sort_by_key(|(name, _)| *name);
        assert_debug_snapshot!(durations, @r#"
        [
            (
                "mod::test_legacy",
                24ms,
            ),
            (
                "mod::test_slow",
                42ms,
            ),
        ]
        "#);
        assert_debug_snapshot!(results.test_cases, @r#"
        [
            TestCaseResult {
                module_name: "mod",
                name: "test_slow(value=1)",
                full_name: "mod::test_slow(value=1)",
                outcome: Failed {
                    diagnostic: RenderedDiagnostic {
                        code: "interrupted",
                        severity: Error,
                        message: "Test `mod::test_slow(value=1)` was interrupted",
                        rendered: "error[interrupted]: Test `mod::test_slow(value=1)` was interrupted\n",
                        colored_rendered: None,
                    },
                    related: [],
                },
                duration: 42ms,
                retry: None,
                captured_output: None,
                attempts: [],
            },
            TestCaseResult {
                module_name: "mod",
                name: "test_legacy[value]",
                full_name: "mod::test_legacy[value]",
                outcome: Failed {
                    diagnostic: RenderedDiagnostic {
                        code: "interrupted",
                        severity: Error,
                        message: "Test `mod::test_legacy[value]` was interrupted",
                        rendered: "error[interrupted]: Test `mod::test_legacy[value]` was interrupted\n",
                        colored_rendered: None,
                    },
                    related: [],
                },
                duration: 24ms,
                retry: None,
                captured_output: None,
                attempts: [],
            },
        ]
        "#);
    }
}
