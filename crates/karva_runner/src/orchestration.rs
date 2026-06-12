use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use colored::Colorize;
use crossbeam_channel::{Receiver, TryRecvError};

use crate::shutdown::shutdown_receiver;
use karva_cache::{
    AggregatedResults, CACHE_DIR, RunCache, RunHash, read_last_failed, read_recent_durations,
    write_last_failed as persist_last_failed,
};
use karva_cli::{PartitionSelection, SubTestCommand};
use karva_collector::{CollectedPackage, CollectionSettings};
use karva_logging::Printer;
use karva_logging::time::{format_duration, format_duration_bracketed};
use karva_project::Project;

use crate::binary::find_karva_worker_binary;
use crate::collection::ParallelCollector;
use crate::partition::{Partition, TestOrdering, partition_collected_tests};
use crate::worker_args::{WorkerSpawn, worker_command};

/// Width that result labels (`PASS`, `FAIL`, `SIGINT`) are right-padded to so
/// columns align. Mirrors the constant in `karva_diagnostic::reporter`.
const LABEL_COLUMN_WIDTH: usize = 12;

/// How `wait_for_completion` exited.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WaitOutcome {
    /// Every worker exited on its own.
    AllCompleted,
    /// Ctrl+C was received; remaining workers must be killed.
    Cancelled,
    /// A worker hit the fail-fast budget; remaining workers must be killed.
    FailFast,
    /// The run timeout elapsed before the workers finished.
    TimedOut,
}

#[derive(Debug)]
struct Worker {
    id: usize,
    child: Child,
    start_time: Instant,
}

impl Worker {
    fn new(id: usize, child: Child) -> Self {
        Self {
            id,
            child,
            start_time: Instant::now(),
        }
    }

    fn duration(&self) -> Duration {
        self.start_time.elapsed()
    }
}

#[derive(Default, Debug)]
struct WorkerManager {
    workers: Vec<Worker>,
}

struct InFlightTest {
    worker_id: usize,
    name: Option<String>,
    elapsed: Duration,
}

struct InterruptedTest {
    name: String,
    duration: Duration,
}

impl WorkerManager {
    fn spawn(&mut self, worker_id: usize, child: Child) {
        self.workers.push(Worker::new(worker_id, child));
    }

    /// Wait for all workers to complete.
    ///
    /// Returns early if a message is received on `shutdown_rx`, if the cache
    /// contains a fail-fast signal indicating a worker encountered a test
    /// failure, or if `run_timeout` elapses. The timeout is measured from the
    /// start of this call (test execution); collection happens beforehand.
    fn wait_for_completion(
        &mut self,
        shutdown_rx: Option<&Receiver<()>>,
        cache: Option<&RunCache>,
        run_timeout: Option<Duration>,
    ) -> WaitOutcome {
        if self.workers.is_empty() {
            return WaitOutcome::AllCompleted;
        }

        tracing::info!(
            "Waiting for {} workers to complete (Ctrl+C to cancel)",
            self.workers.len()
        );

        let start = Instant::now();

        loop {
            if let Some(rx) = shutdown_rx {
                match rx.try_recv() {
                    Ok(()) | Err(TryRecvError::Disconnected) => {
                        tracing::info!("Shutdown requested — stopping remaining workers");
                        return WaitOutcome::Cancelled;
                    }
                    Err(TryRecvError::Empty) => {}
                }
            }

            if let Some(cache) = cache
                && cache.has_fail_fast_signal()
            {
                tracing::info!("Fail-fast signal received — stopping remaining workers");
                return WaitOutcome::FailFast;
            }

            if let Some(timeout) = run_timeout
                && start.elapsed() >= timeout
            {
                tracing::info!("Run timeout exceeded — stopping remaining workers");
                return WaitOutcome::TimedOut;
            }

            self.workers
                .retain_mut(|worker| match worker.child.try_wait() {
                    Ok(Some(status)) => {
                        if status.success() {
                            tracing::info!(
                                "Worker {} completed successfully in {}",
                                worker.id,
                                format_duration(worker.duration()),
                            );
                        } else {
                            tracing::error!(
                                "Worker {} failed with exit code {} in {}",
                                worker.id,
                                status.code().unwrap_or(-1),
                                format_duration(worker.duration()),
                            );
                        }
                        false
                    }
                    Ok(None) => true,
                    Err(e) => {
                        tracing::error!("Error waiting on worker {}: {}", worker.id, e);
                        false
                    }
                });

            if self.workers.is_empty() {
                tracing::info!("All workers completed");
                return WaitOutcome::AllCompleted;
            }

            std::thread::sleep(WORKER_POLL_INTERVAL);
        }
    }

    /// Kill and wait on any remaining worker processes.
    ///
    /// Uses two separate loops: the first sends kill signals to all workers
    /// immediately, and the second reaps them. This ensures every worker
    /// receives the signal without waiting for earlier ones to exit first.
    fn kill_remaining(&mut self) {
        for worker in &mut self.workers {
            if let Err(err) = worker.child.kill() {
                tracing::warn!(
                    worker_id = worker.id,
                    "failed to kill worker process: {err}"
                );
            }
        }
        for worker in &mut self.workers {
            if let Err(err) = worker.child.wait() {
                tracing::warn!(
                    worker_id = worker.id,
                    "failed to wait for worker process: {err}"
                );
            }
        }
    }

    /// Stop remaining workers and emit nextest-style cancellation lines.
    ///
    /// Each worker writes a `current_test.json` file at the start of every
    /// test and removes it when the test finishes. We read those files
    /// *before* killing — once we kill the worker, that file may be removed
    /// by an in-flight finalizer or simply lost — and remember a
    /// `(worker_id, test name, test start time)` snapshot for each.
    ///
    /// Workers are killed and reaped before we print so any in-flight
    /// `PASS`/`FAIL` lines they were writing to the inherited stdout land
    /// before our banner; otherwise the cancellation block interleaves
    /// with worker output. A short settle pause lets any kernel-buffered
    /// writes drain.
    fn cancel_and_kill(&mut self, printer: Printer, cache: &RunCache) -> Vec<InterruptedTest> {
        if self.workers.is_empty() {
            return Vec::new();
        }

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
            .unwrap_or(0);

        let in_flight: Vec<_> = self
            .workers
            .iter()
            .map(|worker| {
                let current = match cache.read_current_test(worker.id) {
                    Ok(current) => current,
                    Err(err) => {
                        tracing::warn!(
                            worker_id = worker.id,
                            "failed to read in-flight test state: {err}"
                        );
                        None
                    }
                };
                let elapsed = current
                    .as_ref()
                    .map(|current| elapsed_since_start(now_ms, current.start_unix_ms, worker.id))
                    .unwrap_or(Duration::ZERO);
                InFlightTest {
                    worker_id: worker.id,
                    name: current.map(|c| c.name),
                    elapsed,
                }
            })
            .collect();

        let running_tests = in_flight.iter().filter(|test| test.name.is_some()).count();
        let test_label = if running_tests == 1 { "test" } else { "tests" };

        for worker in &mut self.workers {
            if let Err(err) = worker.child.kill() {
                tracing::warn!(
                    worker_id = worker.id,
                    "failed to kill worker process: {err}"
                );
            }
        }
        for worker in &mut self.workers {
            if let Err(err) = worker.child.wait() {
                tracing::warn!(
                    worker_id = worker.id,
                    "failed to wait for worker process: {err}"
                );
            }
        }
        std::thread::sleep(STDOUT_SETTLE);

        let mut stdout = printer.stream_for_test_result().lock();
        let cancel_label = "Cancelling".yellow().bold();
        let interrupt_label = "interrupt".yellow().bold();
        if let Err(err) = writeln!(
            stdout,
            "  {cancel_label} due to {interrupt_label}: {running_tests} {test_label} still running"
        ) {
            tracing::warn!("failed to write cancellation banner: {err}");
        }

        let label = "SIGINT".yellow().bold();
        let padding = " ".repeat(LABEL_COLUMN_WIDTH.saturating_sub("SIGINT".len()));
        for test in &in_flight {
            let duration_str = format_duration_bracketed(test.elapsed);
            match &test.name {
                Some(name) => {
                    let colored = format_in_flight_test(name);
                    if let Err(err) = writeln!(stdout, "{padding}{label} {duration_str} {colored}")
                    {
                        tracing::warn!("failed to write interrupted test line: {err}");
                    }
                }
                None => {
                    if let Err(err) = writeln!(
                        stdout,
                        "{padding}{label} {duration_str} worker {} (between tests)",
                        test.worker_id
                    ) {
                        tracing::warn!("failed to write interrupted worker line: {err}");
                    }
                }
            }
        }

        in_flight
            .into_iter()
            .filter_map(|test| {
                test.name.map(|name| InterruptedTest {
                    name,
                    duration: test.elapsed,
                })
            })
            .collect()
    }
}

fn elapsed_since_start(now_ms: u64, start_ms: u64, worker_id: usize) -> Duration {
    let Some(elapsed_ms) = now_ms.checked_sub(start_ms) else {
        tracing::warn!(
            worker_id,
            start_unix_ms = start_ms,
            now_unix_ms = now_ms,
            "in-flight test start time is in the future"
        );
        return Duration::ZERO;
    };
    Duration::from_millis(elapsed_ms)
}

/// Render a `module::function[params]` test name as it was serialised by
/// the worker (`QualifiedTestName::Display`), colouring the module cyan
/// and the function blue+bold to match the per-test result line format.
fn format_in_flight_test(name: &str) -> String {
    if let Some((module, rest)) = name.split_once("::") {
        let module = module.cyan();
        let rest = rest.blue().bold();
        format!("{module}::{rest}")
    } else {
        name.blue().bold().to_string()
    }
}

pub struct ParallelTestConfig {
    pub num_workers: usize,
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
    /// Ordering strategy for partition inputs. Normal runs shuffle tests
    /// without duration history to avoid sticky first-run imbalance; benchmarks
    /// use stable ordering for deterministic inputs.
    pub test_ordering: TestOrdering,
}

/// Spawn worker processes for each partition
///
/// Creates a worker process for each non-empty partition, passing the appropriate
/// subset of tests and command-line arguments to each worker.
fn spawn_workers(spawn: &WorkerSpawn, partitions: &[Partition]) -> Result<WorkerManager> {
    let mut worker_manager = WorkerManager::default();

    for (worker_id, partition) in partitions.iter().enumerate() {
        if partition.tests().is_empty() {
            tracing::debug!("Skipping worker {} with no tests", worker_id);
            continue;
        }

        let child = worker_command(spawn, worker_id, partition)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn karva-worker process")?;

        tracing::info!(
            "Worker {} spawned with {} tests",
            worker_id,
            partition.tests().len()
        );

        worker_manager.spawn(worker_id, child);
    }

    Ok(worker_manager)
}

/// Collect tests from the project without executing them.
pub fn collect_tests(project: &Project) -> Result<CollectedPackage> {
    let mut test_paths = Vec::new();

    for path in project.test_paths() {
        match path {
            Ok(path) => test_paths.push(path),
            Err(err) => return Err(err.into()),
        }
    }

    tracing::debug!(path_count = test_paths.len(), "Found test paths");

    let collection_settings = CollectionSettings {
        python_version: project.metadata().python_version(),
        test_function_prefix: &project.settings().test().test_function_prefix,
        respect_ignore_files: project.settings().src().respect_ignore_files,
        collect_fixtures: false,
    };

    let collector = ParallelCollector::new(project.cwd(), collection_settings);

    let collection_start_time = std::time::Instant::now();

    let collected = collector.collect_all(test_paths)?;

    tracing::info!(
        "Collected all tests in {}",
        format_duration(collection_start_time.elapsed())
    );

    Ok(collected)
}

/// Aggregated outputs of a parallel test run.
pub struct RunOutput {
    /// Test results merged across all workers.
    pub results: AggregatedResults,
    /// Paths to per-worker coverage files written during the run. Empty when
    /// coverage was disabled. The caller hands this to
    /// [`karva_coverage::combine_and_report`] to render the coverage table at
    /// the right point in its output sequence (after the test summary).
    pub coverage_files: Vec<Utf8PathBuf>,
    /// Whether the run was stopped because the configured run timeout elapsed.
    pub timed_out: bool,
}

pub fn run_parallel_tests(
    project: &Project,
    config: &ParallelTestConfig,
    args: &SubTestCommand,
    printer: Printer,
) -> Result<RunOutput> {
    // Install the Ctrl+C handler before any potentially long-running work
    // (collection, partitioning, worker spawn). Otherwise an early SIGINT
    // hits the default disposition and the run terminates silently with no
    // cancellation banner.
    let shutdown_rx = if config.create_ctrlc_handler {
        Some(shutdown_receiver())
    } else {
        None
    };

    let collected = collect_tests(project)?;

    let total_tests = collected.test_count();
    let max_useful_workers = total_tests.div_ceil(MIN_TESTS_PER_WORKER).max(1);
    let num_workers = config.num_workers.min(max_useful_workers);

    if num_workers < config.num_workers {
        tracing::info!(
            total_tests,
            requested_workers = config.num_workers,
            capped_workers = num_workers,
            "Capped worker count to avoid underutilized workers"
        );
    }

    if total_tests > 0 {
        let mut stdout = printer.stream_for_test_result().lock();
        let label = format!("{:>12}", "Starting").green().bold();
        let test_label = if total_tests == 1 { "test" } else { "tests" };
        let worker_label = if num_workers == 1 {
            "worker"
        } else {
            "workers"
        };
        let total_tests_bold = total_tests.to_string().bold();
        let num_workers_bold = num_workers.to_string().bold();
        if let Err(err) = writeln!(
            stdout,
            "{label} {total_tests_bold} {test_label} across {num_workers_bold} {worker_label}"
        ) {
            tracing::warn!("failed to write test start line: {err}");
        }
    }

    tracing::debug!(num_workers, "Partitioning tests");

    let cache_dir = project.cwd().join(CACHE_DIR);

    let previous_durations = previous_durations(&cache_dir, config.no_cache);

    if !previous_durations.is_empty() {
        tracing::debug!(
            "Found {} previous test durations to guide partitioning",
            previous_durations.len()
        );
    }

    let last_failed_set = last_failed_set(&cache_dir, config.last_failed);

    let partitions = partition_collected_tests(
        &collected,
        num_workers,
        &previous_durations,
        &last_failed_set,
        config.partition,
        config.test_ordering,
    );

    let run_hash = RunHash::current_time();
    let cache = RunCache::new(&cache_dir, &run_hash);

    tracing::info!("Spawning {} workers", partitions.len());

    let worker_binary = find_karva_worker_binary(project.cwd())?;
    let spawn = WorkerSpawn {
        project,
        cache_dir: &cache_dir,
        cache: &cache,
        run_hash: &run_hash,
        args,
        num_workers,
        profile: config.profile.as_deref().unwrap_or("default"),
        worker_binary: &worker_binary,
        coverage_enabled: !project.settings().coverage().sources.is_empty(),
    };
    let mut worker_manager = spawn_workers(&spawn, &partitions)?;

    let max_fail_cache = project.settings().max_fail().has_limit().then_some(&cache);

    let outcome = worker_manager.wait_for_completion(
        shutdown_rx,
        max_fail_cache,
        project.settings().test().run_timeout,
    );
    let interrupted_tests = if outcome == WaitOutcome::Cancelled {
        worker_manager.cancel_and_kill(printer, &cache)
    } else {
        worker_manager.kill_remaining();
        Vec::new()
    };

    let timed_out = outcome == WaitOutcome::TimedOut;

    let mut results = cache.aggregate_results()?;
    for test in interrupted_tests {
        results.register_interrupted_test(&test.name, test.duration);
    }

    if !config.no_cache {
        write_last_failed(&cache_dir, &results.failed_tests);
    }

    let coverage_files = if project.settings().coverage().sources.is_empty() {
        Vec::new()
    } else {
        cache.coverage_files()?
    };

    Ok(RunOutput {
        results,
        coverage_files,
        timed_out,
    })
}

const MIN_TESTS_PER_WORKER: usize = 5;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(10);
/// Pause after killing workers to let kernel-buffered output drain to
/// stdout before we emit the cancellation banner.
const STDOUT_SETTLE: Duration = Duration::from_millis(50);

fn previous_durations(cache_dir: &Utf8Path, no_cache: bool) -> HashMap<String, Duration> {
    if no_cache {
        return HashMap::new();
    }

    match read_recent_durations(cache_dir) {
        Ok(durations) => durations,
        Err(err) => {
            tracing::warn!("Failed to read previous test durations from cache: {err}");
            HashMap::new()
        }
    }
}

fn last_failed_set(cache_dir: &Utf8Path, enabled: bool) -> HashSet<String> {
    if !enabled {
        return HashSet::new();
    }

    match read_last_failed(cache_dir) {
        Ok(failed) => failed.into_iter().collect(),
        Err(err) => {
            tracing::warn!("Failed to read last-failed cache: {err}");
            HashSet::new()
        }
    }
}

fn write_last_failed(cache_dir: &Utf8Path, failed_tests: &[String]) {
    if let Err(err) = persist_last_failed(cache_dir, failed_tests) {
        tracing::warn!("Failed to write last-failed cache: {err}");
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::elapsed_since_start;

    #[test]
    fn elapsed_since_start_calculates_duration() {
        assert_eq!(
            elapsed_since_start(1_500, 1_000, 0),
            Duration::from_millis(500)
        );
    }

    #[test]
    fn elapsed_since_start_handles_future_start_time() {
        assert_eq!(elapsed_since_start(1_000, 1_500, 0), Duration::ZERO);
    }
}
