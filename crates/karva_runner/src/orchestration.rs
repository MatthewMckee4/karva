use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use crossbeam_channel::{Receiver, TryRecvError};

use crate::shutdown::shutdown_receiver;
use karva_cache::{AggregatedResults, CACHE_DIR, Cache, RunHash, read_recent_durations};
use karva_cli::SubTestCommand;
use karva_collector::CollectionSettings;
use karva_metadata::ProjectSettings;
use karva_project::{Db, ProjectDatabase};
use karva_system::time::format_duration;
use karva_system::{venv_binary, venv_binary_from_active_env};

use crate::collection::ParallelCollector;
use crate::partition::{Partition, partition_collected_tests};

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
    /// Returns early if a message is received on `shutdown_rx`.
    fn wait_for_completion(&mut self, shutdown_rx: Option<&Receiver<()>>) {
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

            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Kill and wait on any remaining worker processes.
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
}

/// Spawn worker processes for each partition
///
/// Creates a worker process for each non-empty partition, passing the appropriate
/// subset of tests and command-line arguments to each worker.
fn spawn_workers(
    db: &ProjectDatabase,
    partitions: &[Partition],
    cache_dir: &Utf8PathBuf,
    run_hash: &RunHash,
    args: &SubTestCommand,
) -> Result<WorkerManager> {
    let core_binary = find_karva_core_binary(&db.system().current_directory().to_path_buf())?;
    let mut worker_manager = WorkerManager::default();

    for (worker_id, partition) in partitions.iter().enumerate() {
        if partition.tests().is_empty() {
            tracing::debug!("Skipping worker {} with no tests", worker_id);
            continue;
        }

        let mut cmd = Command::new(&core_binary);
        cmd.arg("--cache-dir")
            .arg(cache_dir)
            .arg("--run-hash")
            .arg(run_hash.inner())
            .arg("--worker-id")
            .arg(worker_id.to_string())
            .current_dir(db.system().current_directory())
            // Ensure python does not buffer output
            .env("PYTHONUNBUFFERED", "1");

        for path in partition.tests() {
            cmd.arg(path);
        }

        cmd.args(inner_cli_args(db.project().settings(), args));

        let child = cmd
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .context("Failed to spawn karva_core worker process")?;

        tracing::info!(
            "Worker {} spawned with {} tests",
            worker_id,
            partition.tests().len()
        );

        worker_manager.spawn(worker_id, child);
    }

    Ok(worker_manager)
}

pub fn run_parallel_tests(
    db: &ProjectDatabase,
    config: &ParallelTestConfig,
    args: &SubTestCommand,
) -> Result<AggregatedResults> {
    let mut test_paths = Vec::new();

    for path in db.project().test_paths() {
        match path {
            Ok(path) => test_paths.push(path),
            Err(err) => return Err(err.into()),
        }
    }

    tracing::debug!(path_count = test_paths.len(), "Found test paths");

    let collection_settings = CollectionSettings {
        python_version: db.project().metadata().python_version(),
        test_function_prefix: &db.project().settings().test().test_function_prefix,
        respect_ignore_files: db.project().settings().src().respect_ignore_files,
        collect_fixtures: false,
    };

    let collector = ParallelCollector::new(db.system(), collection_settings);

    let collection_start_time = std::time::Instant::now();

    let collected = collector.collect_all(test_paths)?;

    tracing::info!(
        "Collected all tests in {}",
        format_duration(collection_start_time.elapsed())
    );

    tracing::debug!(num_workers = config.num_workers, "Partitioning tests");

    let cache_dir = db.system().current_directory().join(CACHE_DIR);

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

    let partitions = partition_collected_tests(&collected, config.num_workers, &previous_durations);

    let run_hash = RunHash::current_time();

    tracing::info!("Spawning {} workers", partitions.len());

    let mut worker_manager = spawn_workers(db, &partitions, &cache_dir, &run_hash, args)?;

    let shutdown_rx = if config.create_ctrlc_handler {
        Some(shutdown_receiver())
    } else {
        None
    };

    worker_manager.wait_for_completion(shutdown_rx);
    worker_manager.kill_remaining();

    let cache = Cache::new(&cache_dir, &run_hash);
    let result = cache.aggregate_results()?;

    Ok(result)
}

const KARVA_CORE_BINARY_NAME: &str = "karva-core";

/// Find the `karva-core` binary
fn find_karva_core_binary(current_dir: &Utf8PathBuf) -> Result<Utf8PathBuf> {
    if let Ok(path) = which::which(KARVA_CORE_BINARY_NAME) {
        if let Ok(utf8_path) = Utf8PathBuf::try_from(path) {
            tracing::debug!(path = %utf8_path, "Found binary in PATH");
            return Ok(utf8_path);
        }
    }

    if let Some(venv_binary) = venv_binary(KARVA_CORE_BINARY_NAME, current_dir) {
        return Ok(venv_binary);
    }

    if let Some(venv_binary) = venv_binary_from_active_env(KARVA_CORE_BINARY_NAME) {
        return Ok(venv_binary);
    }

    anyhow::bail!("Could not find karva_core binary")
}

fn inner_cli_args(settings: &ProjectSettings, args: &SubTestCommand) -> Vec<String> {
    let mut cli_args = Vec::new();

    if let Some(arg) = args.verbosity.level().cli_arg() {
        cli_args.push(arg);
    }

    if settings.test().fail_fast {
        cli_args.push("--fail-fast");
    }

    if settings.terminal().show_python_output {
        cli_args.push("-s");
    }

    cli_args.push("--output-format");
    cli_args.push(settings.terminal().output_format.as_str());

    if args.no_progress.is_some_and(|no_progress| no_progress) {
        cli_args.push("--no-progress");
    }

    if let Some(color) = args.color {
        cli_args.push("--color");
        cli_args.push(color.as_str());
    }

    if settings.test().try_import_fixtures {
        cli_args.push("--try-import-fixtures");
    }

    let retry = args.retry.map(|retry| retry.to_string());

    if let Some(retry) = retry.as_deref() {
        cli_args.push("--retry");
        cli_args.push(retry);
    }

    let tag_expressions: Vec<String> = args.tag_expressions.clone();
    let name_patterns: Vec<String> = args.name_patterns.clone();

    cli_args
        .iter()
        .map(ToString::to_string)
        .chain(
            tag_expressions
                .iter()
                .flat_map(|expr| vec!["--tag".to_string(), expr.clone()]),
        )
        .chain(
            name_patterns
                .iter()
                .flat_map(|pattern| vec!["--match".to_string(), pattern.clone()]),
        )
        .collect()
}
