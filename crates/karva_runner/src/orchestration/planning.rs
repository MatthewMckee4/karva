//! Collection and cache inputs used to plan one controller run.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::time::Duration;

use anyhow::Result;
use camino::Utf8Path;
use karva_cache::{read_last_failed, read_recent_durations};
use karva_collector::{CollectedPackage, CollectionSettings};
use karva_project::Project;
use karva_python_semantic::TestCacheKey;

use crate::collection::ParallelCollector;

/// Collects tests without enabling fixture discovery.
pub(super) fn collect_tests(project: &Project) -> Result<CollectedPackage> {
    let mut test_paths = Vec::new();
    for path in project.test_paths() {
        test_paths.push(path?);
    }
    tracing::debug!(target: "karva_runner::orchestration", path_count = test_paths.len(), "Found test paths");

    let collection_settings = CollectionSettings {
        python_version: project.metadata().python_version(),
        test_function_prefix: &project.settings().test().test_function_prefix,
        respect_ignore_files: project.settings().src().respect_ignore_files,
        collect_fixtures: false,
        collect_doctests: project.settings().test().doctest_modules,
    };
    let collector = ParallelCollector::new(project.cwd(), collection_settings);
    let collection_start_time = std::time::Instant::now();
    let collected = collector.collect_all(test_paths)?;
    tracing::info!(target: "karva_runner::orchestration",
        "Collected all tests in {}",
        karva_logging::time::format_duration(collection_start_time.elapsed())
    );
    Ok(collected)
}

pub(super) fn previous_durations(
    cache_dir: &Utf8Path,
    no_cache: bool,
) -> HashMap<TestCacheKey, Duration> {
    if no_cache {
        return HashMap::new();
    }
    match read_recent_durations(cache_dir) {
        Ok(durations) => durations,
        Err(err) => {
            tracing::warn!(target: "karva_runner::orchestration", "Failed to read previous test durations from cache: {err}");
            HashMap::new()
        }
    }
}

pub(super) fn last_failed_set(cache_dir: &Utf8Path, enabled: bool) -> HashSet<TestCacheKey> {
    if !enabled {
        return HashSet::new();
    }
    match read_last_failed(cache_dir) {
        Ok(failed) => failed.into_iter().collect(),
        Err(err) => {
            tracing::warn!(target: "karva_runner::orchestration", "Failed to read last-failed cache: {err}");
            HashSet::new()
        }
    }
}

pub(super) fn write_last_failed(cache_dir: &Utf8Path, failed_tests: &BTreeSet<TestCacheKey>) {
    let failed_tests = failed_tests.iter().cloned().collect::<Vec<_>>();
    if let Err(err) = karva_cache::write_last_failed(cache_dir, &failed_tests) {
        tracing::warn!(target: "karva_runner::orchestration", "Failed to write last-failed cache: {err}");
    }
}
