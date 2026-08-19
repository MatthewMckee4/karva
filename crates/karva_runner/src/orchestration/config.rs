//! Public controller configuration and completed-run output.

use camino::Utf8PathBuf;
use karva_cli::PartitionSelection;
use karva_diagnostic::AggregatedResults;

use crate::partition::TestOrdering;

/// Controller settings that affect worker count, selection, and lifecycle.
pub struct ParallelTestConfig {
    /// Maximum worker processes before capping against collected test count.
    pub num_workers: usize,

    /// Whether historical durations and last-failed data may be read.
    pub no_cache: bool,

    /// Whether to create a Ctrl+C handler for graceful shutdown.
    ///
    /// When `true`, a signal handler is installed (idempotently) to handle
    /// Ctrl+C and gracefully stop workers. Set to `false` in contexts where
    /// the handler should not be installed (e.g., benchmarks).
    pub create_ctrlc_handler: bool,

    /// When `true`, only tests that failed in the previous run will be executed.
    pub last_failed: bool,

    /// Active configuration profile name. Propagated to workers as
    /// `KARVA_PROFILE`; falls back to `"default"` when `None`.
    pub profile: Option<String>,

    /// When set, restrict the run to the selected slice of collected tests.
    pub partition: Option<PartitionSelection>,

    /// Ordering strategy for partition inputs.
    pub test_ordering: TestOrdering,

    /// Which completed test case bodies the controller retains.
    pub result_retention: TestResultRetention,

    /// How completed test durations are retained.
    pub duration_retention: DurationRetention,
}

/// Controls controller storage for completed test durations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurationRetention {
    /// Keep only compact completion identities for crash recovery.
    Compact,

    /// Keep every duration for cache writes or slowest-test output.
    Retain,
}

/// Controls whether successful non-retried case bodies remain in memory.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TestResultRetention {
    /// Retain failures and retries needed for terminal reporting.
    #[default]
    FailuresAndRetries,

    /// Retain every case for JSON or `JUnit` reports.
    All,
}

/// Aggregated outputs of a parallel test run.
pub struct RunOutput {
    /// Test results merged across all workers.
    pub results: AggregatedResults,

    /// Paths to per-worker coverage files written during the run. Empty when
    /// coverage was disabled.
    pub coverage_files: Vec<Utf8PathBuf>,

    /// Whether the run was stopped because the configured run timeout elapsed.
    pub timed_out: bool,
}
