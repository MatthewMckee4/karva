use std::collections::HashMap;
use std::io::ErrorKind;
use std::time::Duration;

use anyhow::Result;
use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;
use karva_python_semantic::TestCacheKey;

use crate::artifact::{CacheFile, read_json, write_json};
use crate::{RUN_PREFIX, RunHash, WORKER_PREFIX, worker_folder};

/// Resolves run-scoped files that workers must persist, currently coverage only.
pub struct RunArtifacts {
    /// Run-scoped directory containing every worker's artifacts.
    run_dir: Utf8PathBuf,
}

impl RunArtifacts {
    /// Constructs an artifact handle for a specific run within the cache directory.
    pub fn new(cache_dir: &Utf8Path, run_hash: &RunHash) -> Self {
        let run_dir = cache_dir.join(run_hash.to_string());
        Self { run_dir }
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
    if let Some(durations) = read_json(cache_dir, CacheFile::Durations)? {
        return Ok(durations);
    }

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

/// Replaces the duration history used to balance the next test run.
pub fn write_durations(
    cache_dir: &Utf8Path,
    durations: &HashMap<TestCacheKey, Duration>,
) -> Result<()> {
    fs::create_dir_all(cache_dir)?;
    write_json(cache_dir, CacheFile::Durations, durations)
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

    use super::*;
    use camino::Utf8PathBuf;
    use insta::assert_debug_snapshot;

    fn create_cache_with_durations(
        dir: &std::path::Path,
        run_name: &str,
        worker_id: usize,
        durations: &HashMap<TestCacheKey, Duration>,
    ) {
        let worker_dir = dir.join(run_name).join(format!("worker-{worker_id}"));
        fs::create_dir_all(&worker_dir).unwrap();
        let json = serde_json::to_string(durations).unwrap();
        fs::write(worker_dir.join(CacheFile::Durations.filename()), json).unwrap();
    }

    #[test]
    fn read_recent_durations_returns_from_most_recent_run() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();

        let mut old_durations = HashMap::new();
        old_durations.insert(
            TestCacheKey::function_name("test_old"),
            Duration::from_millis(100),
        );
        create_cache_with_durations(tmp.path(), "run-100", 0, &old_durations);

        let mut new_durations = HashMap::new();
        new_durations.insert(
            TestCacheKey::function_name("test_new"),
            Duration::from_millis(200),
        );
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
    fn root_durations_take_precedence_over_legacy_runs() {
        let tmp = tempfile::tempdir().unwrap();
        let cache_dir = Utf8PathBuf::try_from(tmp.path().to_path_buf()).unwrap();
        create_cache_with_durations(
            tmp.path(),
            "run-200",
            0,
            &HashMap::from([(
                TestCacheKey::function_name("legacy"),
                Duration::from_millis(10),
            )]),
        );
        let current = HashMap::from([(
            TestCacheKey::function_name("current"),
            Duration::from_millis(20),
        )]);

        write_durations(&cache_dir, &current).unwrap();

        assert_eq!(read_recent_durations(&cache_dir).unwrap(), current);
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
}
