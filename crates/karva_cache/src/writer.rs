use std::fs;
use std::io::Write;

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};

use crate::models::{RunHash, SerializableStats, sanitize_test_path};

/// Writes test results to the cache directory
///
/// Used by worker processes to persist test results incrementally.
pub struct CacheWriter {
    cache_dir: Utf8PathBuf,
    run_hash: RunHash,
    worker_id: usize,
    worker_dir: Utf8PathBuf,
}

impl CacheWriter {
    /// Create a new cache writer for a specific worker
    ///
    /// # Arguments
    /// * `cache_dir` - The base cache directory (e.g., `.karva-cache`)
    /// * `run_hash` - Unique identifier for this test run
    /// * `worker_id` - The worker ID (0-based index)
    pub fn new(cache_dir: Utf8PathBuf, run_hash: RunHash, worker_id: usize) -> Result<Self> {
        let worker_dir = cache_dir
            .join(run_hash.to_string())
            .join(format!("worker-{}", worker_id));

        tracing::info!(
            cache_dir = %cache_dir,
            run_hash = %run_hash.0,
            worker_id = worker_id,
            worker_dir = %worker_dir,
            "Creating cache writer"
        );

        // Create the worker directory
        fs::create_dir_all(&worker_dir)
            .with_context(|| format!("Failed to create worker directory: {}", worker_dir))?;

        tracing::debug!(worker_dir = %worker_dir, "Worker directory created");

        Ok(Self {
            cache_dir,
            run_hash,
            worker_id,
            worker_dir,
        })
    }

    /// Write test results for a specific test
    ///
    /// # Arguments
    /// * `test_path` - The test identifier (e.g., "tests/test_foo.py::test_example")
    /// * `stats` - Test statistics (passed/failed/skipped counts)
    pub fn write_test_result(
        &self,
        test_path: &str,
        stats: &SerializableStats,
    ) -> Result<()> {
        let test_dir = self.get_test_dir(test_path);

        tracing::debug!(
            test_path = test_path,
            test_dir = %test_dir,
            passed = stats.passed,
            failed = stats.failed,
            skipped = stats.skipped,
            "Writing test result to cache"
        );

        // Create test directory
        fs::create_dir_all(&test_dir)
            .with_context(|| format!("Failed to create test directory: {}", test_dir))?;

        // Write stats.json
        self.write_stats(&test_dir, stats)?;

        tracing::trace!(test_path = test_path, "Test result written successfully");

        Ok(())
    }

    /// Write formatted diagnostics for this worker
    ///
    /// # Arguments
    /// * `diagnostics` - Formatted diagnostic output as a string
    /// * `discovery_diagnostics` - Formatted discovery diagnostic output as a string
    pub fn write_diagnostics(
        &self,
        diagnostics: &str,
        discovery_diagnostics: &str,
    ) -> Result<()> {
        tracing::debug!(
            worker_id = self.worker_id,
            diagnostics_len = diagnostics.len(),
            discovery_diagnostics_len = discovery_diagnostics.len(),
            "Writing worker diagnostics to cache"
        );

        if !diagnostics.is_empty() {
            let diagnostics_path = self.worker_dir.join("diagnostics.txt");
            fs::write(&diagnostics_path, diagnostics)
                .with_context(|| format!("Failed to write diagnostics file: {}", diagnostics_path))?;
        }

        if !discovery_diagnostics.is_empty() {
            let discovery_diagnostics_path = self.worker_dir.join("discovery_diagnostics.txt");
            fs::write(&discovery_diagnostics_path, discovery_diagnostics)
                .with_context(|| format!("Failed to write discovery diagnostics file: {}", discovery_diagnostics_path))?;
        }

        Ok(())
    }

    /// Get the directory path for a specific test
    fn get_test_dir(&self, test_path: &str) -> Utf8PathBuf {
        let sanitized = sanitize_test_path(test_path);
        self.worker_dir.join(sanitized)
    }

    /// Write stats to stats.json
    fn write_stats(&self, test_dir: &Utf8Path, stats: &SerializableStats) -> Result<()> {
        let stats_path = test_dir.join("stats.json");
        let json = serde_json::to_string_pretty(stats).context("Failed to serialize stats")?;

        fs::write(&stats_path, json)
            .with_context(|| format!("Failed to write stats file: {}", stats_path))?;

        Ok(())
    }

    /// Get the worker directory path
    pub const fn worker_dir(&self) -> &Utf8PathBuf {
        &self.worker_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_cache_writer_creation() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        let run_hash = RunHash("run-123-abc".to_string());
        let writer = CacheWriter::new(cache_dir.clone(), run_hash.clone(), 0).unwrap();

        assert_eq!(writer.worker_id, 0);
        assert!(writer.worker_dir.exists());
        assert_eq!(
            writer.worker_dir,
            cache_dir.join("run-123-abc").join("worker-0")
        );
    }

    #[test]
    fn test_write_test_result() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        let run_hash = RunHash("run-123-abc".to_string());
        let writer = CacheWriter::new(cache_dir.clone(), run_hash.clone(), 0).unwrap();

        let stats = SerializableStats {
            passed: 5,
            failed: 2,
            skipped: 1,
        };

        let diagnostics = vec![
            SerializableDiagnostic::new("Test failed")
                .with_severity("Error")
                .with_file(Utf8PathBuf::from("tests/test_foo.py"))
                .with_line(10),
        ];

        writer
            .write_test_result("tests/test_foo.py::test_example", &stats, &diagnostics)
            .unwrap();

        // Verify files were created
        let test_dir = writer.worker_dir.join("tests__test_foo.py___test_example");
        assert!(test_dir.exists());
        assert!(test_dir.join("stats.json").exists());
        assert!(test_dir.join("diagnostics.jsonl").exists());

        // Verify stats content
        let stats_content = fs::read_to_string(test_dir.join("stats.json")).unwrap();
        let read_stats: SerializableStats = serde_json::from_str(&stats_content).unwrap();
        assert_eq!(read_stats, stats);

        // Verify diagnostics content
        let diagnostics_content = fs::read_to_string(test_dir.join("diagnostics.jsonl")).unwrap();
        let lines: Vec<&str> = diagnostics_content.trim().split('\n').collect();
        assert_eq!(lines.len(), 1);

        let read_diagnostic: SerializableDiagnostic = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(read_diagnostic, diagnostics[0]);
    }

    #[test]
    fn test_write_test_result_with_parameters() {
        let temp_dir = TempDir::new().unwrap();
        let cache_dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        let run_hash = RunHash("run-123-abc".to_string());
        let writer = CacheWriter::new(cache_dir.clone(), run_hash.clone(), 0).unwrap();

        let stats = SerializableStats::default();
        let diagnostics = Vec::new();

        writer
            .write_test_result(
                "tests/test_foo.py::test_example[param1-param2]",
                &stats,
                &diagnostics,
            )
            .unwrap();

        // Verify sanitized directory name
        let test_dir = writer
            .worker_dir
            .join("tests__test_foo.py___test_example[param1-param2]");
        assert!(test_dir.exists());
    }
}
