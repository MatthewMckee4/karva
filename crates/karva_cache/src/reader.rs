use std::fs;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use karva_diagnostic::{TestResultKind, TestResultStats};

use crate::models::{RunHash, SerializableStats};

pub struct AggregatedResults {
    pub stats: TestResultStats,
    pub diagnostics: String,
    pub discovery_diagnostics: String,
}

/// Reads and aggregates test results from the cache directory
///
/// Used by the main process to collect results from all worker processes.
pub struct CacheReader {
    cache_dir: Utf8PathBuf,
    run_hash: RunHash,
    run_dir: Utf8PathBuf,
}

impl CacheReader {
    /// Create a new cache reader
    ///
    /// # Arguments
    /// * `cache_dir` - The base cache directory (e.g., `.karva-cache`)
    /// * `run_hash` - Unique identifier for this test run
    pub fn new(cache_dir: Utf8PathBuf, run_hash: RunHash) -> Result<Self> {
        let run_dir = cache_dir.join(run_hash.to_string());

        Ok(Self {
            cache_dir,
            run_hash,
            run_dir,
        })
    }

    /// Aggregate results from all workers
    ///
    /// # Arguments
    /// * `worker_count` - The number of workers that were spawned
    pub fn aggregate_results(&self, worker_count: usize) -> Result<AggregatedResults> {
        tracing::info!(
            run_dir = %self.run_dir,
            worker_count = worker_count,
            "Starting cache aggregation"
        );

        let mut aggregated_stats = SerializableStats::default();
        let mut all_diagnostics = String::new();
        let mut all_discovery_diagnostics = String::new();

        for worker_id in 0..worker_count {
            let worker_dir = self.run_dir.join(format!("worker-{}", worker_id));

            tracing::debug!(
                worker_id = worker_id,
                worker_dir = %worker_dir,
                exists = worker_dir.exists(),
                "Checking worker directory"
            );

            // Skip if worker directory doesn't exist (worker may have crashed before writing)
            if !worker_dir.exists() {
                tracing::warn!(worker_id = worker_id, "Worker directory does not exist");
                continue;
            }

            // Read all test results from this worker
            self.read_worker_results(
                &worker_dir,
                &mut aggregated_stats,
                &mut all_diagnostics,
                &mut all_discovery_diagnostics,
            )?;

            tracing::debug!(
                worker_id = worker_id,
                passed = aggregated_stats.passed,
                failed = aggregated_stats.failed,
                skipped = aggregated_stats.skipped,
                "Aggregated stats so far"
            );
        }

        tracing::info!(
            total_passed = aggregated_stats.passed,
            total_failed = aggregated_stats.failed,
            total_skipped = aggregated_stats.skipped,
            diagnostics_len = all_diagnostics.len(),
            discovery_diagnostics_len = all_discovery_diagnostics.len(),
            "Cache aggregation complete"
        );

        // Convert stats to TestResultStats
        let mut test_stats = TestResultStats::default();
        for _ in 0..aggregated_stats.passed {
            test_stats.add(TestResultKind::Passed);
        }
        for _ in 0..aggregated_stats.failed {
            test_stats.add(TestResultKind::Failed);
        }
        for _ in 0..aggregated_stats.skipped {
            test_stats.add(TestResultKind::Skipped);
        }

        Ok(AggregatedResults {
            stats: test_stats,
            diagnostics: all_diagnostics,
            discovery_diagnostics: all_discovery_diagnostics,
        })
    }

    /// Read results from a single worker directory
    fn read_worker_results(
        &self,
        worker_dir: &Utf8Path,
        aggregated_stats: &mut SerializableStats,
        all_diagnostics: &mut String,
        all_discovery_diagnostics: &mut String,
    ) -> Result<()> {
        tracing::debug!(worker_dir = %worker_dir, "Reading worker results");

        let mut test_count = 0;

        // Iterate through all test directories in this worker
        for entry in fs::read_dir(worker_dir)
            .with_context(|| format!("Failed to read worker directory: {}", worker_dir))?
        {
            let entry = entry.context("Failed to read directory entry")?;
            let test_dir = entry.path();

            if !test_dir.is_dir() {
                continue;
            }

            let test_dir =
                Utf8PathBuf::try_from(test_dir).context("Failed to convert path to UTF-8")?;

            tracing::trace!(test_dir = %test_dir, "Processing test directory");
            test_count += 1;

            // Read stats
            if let Ok(stats) = self.read_stats(&test_dir) {
                tracing::trace!(
                    test_dir = %test_dir,
                    passed = stats.passed,
                    failed = stats.failed,
                    skipped = stats.skipped,
                    "Read stats from test"
                );
                aggregated_stats.merge(&stats);
            } else {
                tracing::warn!(test_dir = %test_dir, "Failed to read stats for test");
            }
        }

        // Read diagnostics from worker-level files
        let diagnostics_path = worker_dir.join("diagnostics.txt");
        if diagnostics_path.exists() {
            match fs::read_to_string(&diagnostics_path) {
                Ok(content) => {
                    all_diagnostics.push_str(&content);
                    tracing::trace!(
                        worker_dir = %worker_dir,
                        len = content.len(),
                        "Read diagnostics"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        worker_dir = %worker_dir,
                        error = %e,
                        "Failed to read diagnostics file"
                    );
                }
            }
        }

        let discovery_diagnostics_path = worker_dir.join("discovery_diagnostics.txt");
        if discovery_diagnostics_path.exists() {
            match fs::read_to_string(&discovery_diagnostics_path) {
                Ok(content) => {
                    all_discovery_diagnostics.push_str(&content);
                    tracing::trace!(
                        worker_dir = %worker_dir,
                        len = content.len(),
                        "Read discovery diagnostics"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        worker_dir = %worker_dir,
                        error = %e,
                        "Failed to read discovery diagnostics file"
                    );
                }
            }
        }

        tracing::debug!(
            worker_dir = %worker_dir,
            test_count = test_count,
            "Completed reading worker results"
        );

        Ok(())
    }

    /// Read stats from a test directory
    fn read_stats(&self, test_dir: &Utf8Path) -> Result<SerializableStats> {
        let stats_path = test_dir.join("stats.json");
        let content = fs::read_to_string(&stats_path)
            .with_context(|| format!("Failed to read stats file: {}", stats_path))?;

        serde_json::from_str(&content).context("Failed to deserialize stats")
    }

    /// Clean up the cache directory for this run
    pub fn cleanup(&self) -> Result<()> {
        if self.run_dir.exists() {
            fs::remove_dir_all(&self.run_dir)
                .with_context(|| format!("Failed to remove cache directory: {}", self.run_dir))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::writer::CacheWriter;
    use tempfile::TempDir;

    #[test]
    fn test_cache_reader_aggregation() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        let run_hash = RunHash("run-123-abc".to_string());

        // Create two workers with different results
        let writer0 = CacheWriter::new(cache_dir.clone(), run_hash.clone(), 0).unwrap();
        let writer1 = CacheWriter::new(cache_dir.clone(), run_hash.clone(), 1).unwrap();

        // Worker 0: 3 passed, 1 failed
        let stats0 = SerializableStats {
            passed: 3,
            failed: 1,
            skipped: 0,
        };
        writer0
            .write_test_result("test1.py::test_a", &stats0)
            .unwrap();

        // Worker 1: 2 passed, 1 skipped
        let stats1 = SerializableStats {
            passed: 2,
            failed: 0,
            skipped: 1,
        };
        writer1
            .write_test_result("test2.py::test_b", &stats1)
            .unwrap();

        // Read and aggregate
        let reader = CacheReader::new(cache_dir.clone(), run_hash.clone()).unwrap();
        let result = reader.aggregate_results(2).unwrap();

        // Verify aggregated stats
        assert_eq!(result.stats.passed(), 5);
        assert_eq!(result.stats.failed(), 1);
        assert_eq!(result.stats.skipped(), 1);
    }

    #[test]
    fn test_cache_reader_with_diagnostics() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        let run_hash = RunHash("run-123-abc".to_string());
        let writer = CacheWriter::new(cache_dir.clone(), run_hash.clone(), 0).unwrap();

        let stats = SerializableStats::default();
        writer
            .write_test_result("test.py::test_example", &stats)
            .unwrap();

        // Write diagnostics
        writer
            .write_diagnostics("some diagnostic output", "discovery diagnostic output")
            .unwrap();

        // Read and verify
        let reader = CacheReader::new(cache_dir.clone(), run_hash.clone()).unwrap();
        let result = reader.aggregate_results(1).unwrap();

        assert_eq!(result.diagnostics, "some diagnostic output");
        assert_eq!(result.discovery_diagnostics, "discovery diagnostic output");
    }

    #[test]
    fn test_cache_cleanup() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        let run_hash = RunHash("run-123-abc".to_string());
        let writer = CacheWriter::new(cache_dir.clone(), run_hash.clone(), 0).unwrap();

        writer
            .write_test_result("test.py::test_example", &SerializableStats::default())
            .unwrap();

        let reader = CacheReader::new(cache_dir.clone(), run_hash.clone()).unwrap();
        assert!(reader.run_dir.exists());

        reader.cleanup().unwrap();
        assert!(!reader.run_dir.exists());
    }
}
