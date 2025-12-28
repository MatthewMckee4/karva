use std::collections::HashSet;
use std::fmt::Write as _;

use anyhow::Result;
use karva_cache::{CacheWriter, models::SerializableStats};
use karva_diagnostic::{IndividualTestResultKind, Reporter, TestResultKind};
use karva_project::{Db, ProjectDatabase};
use karva_python_semantic::QualifiedTestName;
use ruff_db::diagnostic::{DisplayDiagnosticConfig, DisplayDiagnostics};

use crate::runner::TestRunner;

/// A reporter that writes test results to cache
struct CacheReporter<'a> {
    cache_writer: &'a CacheWriter,
    test_filter: HashSet<String>,
}

impl<'a> CacheReporter<'a> {
    fn new(cache_writer: &'a CacheWriter, test_filter: HashSet<String>) -> Self {
        Self {
            cache_writer,
            test_filter,
        }
    }

    fn result_to_stats(result_kind: &IndividualTestResultKind) -> SerializableStats {
        match TestResultKind::from(result_kind.clone()) {
            TestResultKind::Passed => SerializableStats {
                passed: 1,
                failed: 0,
                skipped: 0,
            },
            TestResultKind::Failed => SerializableStats {
                passed: 0,
                failed: 1,
                skipped: 0,
            },
            TestResultKind::Skipped => SerializableStats {
                passed: 0,
                failed: 0,
                skipped: 1,
            },
        }
    }
}

impl Reporter for CacheReporter<'_> {
    fn report_test_case_result(
        &self,
        test_name: &QualifiedTestName,
        result_kind: IndividualTestResultKind,
    ) {
        tracing::debug!(
            test_name = test_name.function_name().to_string(),
            in_filter = self
                .test_filter
                .contains(&test_name.function_name().to_string()),
            "Received test case result"
        );

        // Only write results for tests in our filter set
        if !self
            .test_filter
            .contains(&test_name.function_name().to_string())
        {
            tracing::warn!(
                test_name = test_name.function_name().to_string(),
                filter_count = self.test_filter.len(),
                "Skipping test not in filter"
            );
            return;
        }

        let result_type = match &result_kind {
            IndividualTestResultKind::Passed => "passed",
            IndividualTestResultKind::Failed => "failed",
            IndividualTestResultKind::Skipped { .. } => "skipped",
        };

        tracing::debug!(
            test_name = test_name.function_name().to_string(),
            result = result_type,
            "Reporting test result"
        );

        let stats = Self::result_to_stats(&result_kind);

        // Write to cache (ignore errors since Reporter trait doesn't allow Result)
        if let Err(e) = self.cache_writer.write_test_result(
            &test_name.function_name().to_string(),
            &stats,
        ) {
            tracing::error!(test_name = test_name.function_name().to_string(), error = %e, "Failed to write test result to cache");
        } else {
            tracing::trace!(
                test_name = test_name.function_name().to_string(),
                "Test result written to cache"
            );
        }
    }
}

/// Execute a list of test paths and write results to cache
pub fn execute_test_paths(
    db: &ProjectDatabase,
    test_paths: &[String],
    cache_writer: &CacheWriter,
    _fail_fast: bool,
    _show_output: bool,
) -> Result<i32> {
    tracing::info!(
        test_count = test_paths.len(),
        worker_dir = %cache_writer.worker_dir(),
        "Worker starting test execution"
    );

    // Convert test paths to a set for quick lookup
    let test_path_set: HashSet<String> = test_paths.iter().cloned().collect();

    tracing::debug!(
        filter_count = test_path_set.len(),
        test_paths = ?test_path_set,
        "Creating cache reporter with filter"
    );
    // Create a reporter that writes to cache, filtering to only our test paths
    let reporter = CacheReporter::new(cache_writer, test_path_set);

    tracing::debug!("Running test pipeline");
    // Run the full test pipeline with our cache reporter
    let result = db.test_with_reporter(&reporter);

    tracing::info!(
        passed = result.stats().passed(),
        failed = result.stats().failed(),
        skipped = result.stats().skipped(),
        success = result.is_success(),
        "Worker completed test execution"
    );

    // Format and write diagnostics
    tracing::debug!("Formatting and writing diagnostics");
    let diagnostic_format = db.project().settings().terminal().output_format.into();
    let config = DisplayDiagnosticConfig::default()
        .format(diagnostic_format)
        .color(false); // Disable color for cached output

    let mut diagnostics_buffer = String::new();
    let mut discovery_diagnostics_buffer = String::new();

    if !result.diagnostics().is_empty() {
        write!(
            &mut diagnostics_buffer,
            "{}",
            DisplayDiagnostics::new(db, &config, result.diagnostics())
        )?;
    }

    if !result.discovery_diagnostics().is_empty() {
        write!(
            &mut discovery_diagnostics_buffer,
            "{}",
            DisplayDiagnostics::new(db, &config, result.discovery_diagnostics())
        )?;
    }

    cache_writer.write_diagnostics(&diagnostics_buffer, &discovery_diagnostics_buffer)?;
    tracing::debug!("Diagnostics written to cache");

    // Always return success exit code - test failures are not worker failures
    // Only serious errors (database issues, etc.) will propagate as Err
    Ok(0)
}
