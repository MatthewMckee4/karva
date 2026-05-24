use std::collections::HashSet;
use std::fmt::Write;
use std::process::{Child, ChildStdout, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use colored::Colorize;
use crossbeam_channel::{Receiver, TryRecvError};

use crate::shutdown::shutdown_receiver;
use karva_cache::{
    AggregatedResults, CACHE_DIR, RunCache, RunHash, read_last_failed, read_recent_durations,
    write_last_failed,
};
use karva_cli::{PartitionSelection, SubTestCommand};
use karva_collector::{CollectedPackage, CollectionSettings};
use karva_logging::Printer;
use karva_logging::time::format_duration;
use karva_project::Project;

use crate::binary::find_karva_worker_binary;
use crate::collection::ParallelCollector;
use crate::partition::{Partition, partition_collected_tests};
use crate::progress::{OutputDrain, WorkerPipes};
use crate::worker_args::{WorkerSpawn, worker_command};

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

impl WorkerManager {
    fn spawn(&mut self, worker_id: usize, child: Child) {
        self.workers.push(Worker::new(worker_id, child));
    }

    /// Wait for all workers to complete.
    /// Returns early if a message is received on `shutdown_rx` or if the cache
    /// contains a fail-fast signal indicating a worker encountered a test failure.
    fn wait_for_completion(
        &mut self,
        shutdown_rx: Option<&Receiver<()>>,
        cache: Option<&RunCache>,
    ) {
        if self.workers.is_empty() {
            return;
        }

        tracing::info!(
            "Waiting for {} workers to complete (Ctrl+C to cancel)",
            self.workers.len()
        );

        loop {
            if let Some(rx) = shutdown_rx {
                match rx.try_recv() {
                    Ok(()) | Err(TryRecvError::Disconnected) => {
                        tracing::info!("Shutdown requested — stopping remaining workers");
                        break;
                    }
                    Err(TryRecvError::Empty) => {}
                }
            }

            if let Some(cache) = cache
                && cache.has_fail_fast_signal()
            {
                tracing::info!("Fail-fast signal received — stopping remaining workers");
                break;
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
                break;
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
            let _ = worker.child.kill();
        }
        for worker in &mut self.workers {
            let _ = worker.child.wait();
        }
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
}

/// Spawn worker processes for each partition
///
/// Creates a worker process for each non-empty partition, passing the appropriate
/// subset of tests and command-line arguments to each worker.
fn spawn_workers(
    spawn: &WorkerSpawn,
    partitions: &[Partition],
) -> Result<(WorkerManager, Vec<WorkerPipes>)> {
    let mut worker_manager = WorkerManager::default();
    let mut pipes: Vec<WorkerPipes> = Vec::new();

    for (worker_id, partition) in partitions.iter().enumerate() {
        if partition.tests().is_empty() {
            tracing::debug!("Skipping worker {} with no tests", worker_id);
            continue;
        }

        let mut child = worker_command(spawn, worker_id, partition)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn karva-worker process")?;

        let stdout: Option<ChildStdout> = child.stdout.take();

        tracing::info!(
            "Worker {} spawned with {} tests",
            worker_id,
            partition.tests().len()
        );

        worker_manager.spawn(worker_id, child);
        pipes.push(WorkerPipes { stdout });
    }

    Ok((worker_manager, pipes))
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
}

pub fn run_parallel_tests(
    project: &Project,
    config: &ParallelTestConfig,
    args: &SubTestCommand,
    printer: Printer,
) -> Result<RunOutput> {
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
        writeln!(
            stdout,
            "{label} {total_tests_bold} {test_label} across {num_workers_bold} {worker_label}"
        )
        .ok();
    }

    tracing::debug!(num_workers, "Partitioning tests");

    let cache_dir = project.cwd().join(CACHE_DIR);

    // Read durations from the most recent run to optimize partitioning
    let previous_durations = if config.no_cache {
        std::collections::HashMap::new()
    } else {
        read_recent_durations(&cache_dir).unwrap_or_default()
    };

    if !previous_durations.is_empty() {
        tracing::debug!(
            "Found {} previous test durations to guide partitioning",
            previous_durations.len()
        );
    }

    let last_failed_set: HashSet<String> = if config.last_failed {
        read_last_failed(&cache_dir)
            .unwrap_or_default()
            .into_iter()
            .collect()
    } else {
        HashSet::new()
    };

    let partitions = partition_collected_tests(
        &collected,
        num_workers,
        &previous_durations,
        &last_failed_set,
        config.partition,
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
    let (mut worker_manager, pipes) = spawn_workers(&spawn, &partitions)?;

    let shutdown_rx = if config.create_ctrlc_handler {
        Some(shutdown_receiver())
    } else {
        None
    };

    let max_fail_cache = project.settings().max_fail().has_limit().then_some(&cache);

    let drain = OutputDrain::start(
        project.settings().terminal().show_progress,
        total_tests as u64,
        num_workers,
        cache.clone(),
        pipes,
    );

    worker_manager.wait_for_completion(shutdown_rx, max_fail_cache);
    worker_manager.kill_remaining();

    drain.finish();

    let results = cache.aggregate_results()?;

    if !config.no_cache {
        let _ = write_last_failed(&cache_dir, &results.failed_tests);
    }

    let coverage_files = if project.settings().coverage().sources.is_empty() {
        Vec::new()
    } else {
        cache.coverage_files()?
    };

    Ok(RunOutput {
        results,
        coverage_files,
    })
}

const MIN_TESTS_PER_WORKER: usize = 5;
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(10);
