use std::fmt::Write;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use camino::Utf8PathBuf;
use karva_cache::{
    AggregatedResults, CACHE_DIR, CacheReader, RunHash, reader::read_recent_durations,
};
use karva_cli::{OutputFormat, SubTestCommand};
use karva_collector::ParallelCollector;
use karva_logging::Printer;
use karva_project::{Db, ProjectDatabase};
use karva_system::time::format_duration;
use karva_system::venv_binary;

use crate::partition::{Partition, partition_collected_tests};

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

#[derive(Default)]
struct WorkerManager {
    workers: Vec<Worker>,
}

impl WorkerManager {
    fn spawn(&mut self, worker_id: usize, child: Child) {
        self.workers.push(Worker::new(worker_id, child));
    }

    fn wait_all(&mut self) -> usize {
        let num_workers = self.workers.len();

        tracing::info!("All workers spawned, waiting for completion");

        while !self.workers.is_empty() {
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
                                "Worker {} exited with non-zero status {} in {}",
                                worker.id,
                                status.code().unwrap_or(-1),
                                format_duration(worker.duration()),
                            );
                        }
                        false
                    }
                    Ok(None) => true,
                    Err(_) => false,
                });
        }

        num_workers
    }
}

pub struct ParallelTestConfig {
    pub num_workers: usize,
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
    printer: Printer,
) -> Result<WorkerManager> {
    let core_binary = find_karva_core_binary(&db.system().current_directory().to_path_buf())?;
    let mut worker_manager = WorkerManager::default();

    for (worker_id, partition) in partitions.iter().enumerate() {
        if partition.tests().is_empty() {
            tracing::debug!(worker_id = worker_id, "Skipping worker with no tests");
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

        cmd.args(args.into_cli_args());

        let child = cmd
            .stdout(printer.stream_for_details())
            .stderr(printer.stream_for_details())
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
    printer: Printer,
) -> Result<bool> {
    let start_time = std::time::Instant::now();

    let mut test_paths = Vec::new();

    for path in db.project().test_paths() {
        match path {
            Ok(path) => test_paths.push(path),
            Err(err) => {
                anyhow::bail!(err);
            }
        }
    }

    tracing::debug!(path_count = test_paths.len(), "Found test paths");

    let collector = ParallelCollector::new(
        db.system(),
        db.project().metadata(),
        db.project().settings(),
    );

    let collection_start_time = std::time::Instant::now();

    let collected = collector.collect_all(test_paths);

    tracing::info!(
        "Collected all tests in {}",
        format_duration(collection_start_time.elapsed())
    );

    tracing::debug!("Attempting to create {} workers", config.num_workers);

    let cache_dir = db.project().cwd().join(CACHE_DIR);

    // Read durations from the most recent run to optimize partitioning
    let previous_durations = read_recent_durations(&cache_dir).unwrap_or_default();

    if !previous_durations.is_empty() {
        tracing::debug!(
            "Found {} previous test durations to guide partitioning",
            previous_durations.len()
        );
    }

    let partitions = partition_collected_tests(&collected, config.num_workers, &previous_durations);

    let run_hash = RunHash::current_time();

    tracing::info!("Attempting to spawn {} workers", partitions.len());

    let mut worker_manager = spawn_workers(db, &partitions, &cache_dir, &run_hash, args, printer)?;

    let num_workers = worker_manager.wait_all();

    let result = print_test_output(
        printer,
        start_time,
        &cache_dir,
        &run_hash,
        num_workers,
        args.output_format.as_ref(),
    )?;

    Ok(result.stats.is_success() && result.discovery_diagnostics.is_empty())
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

    let venv_binary = venv_binary(KARVA_CORE_BINARY_NAME, current_dir);

    if let Some(venv_binary) = venv_binary {
        if venv_binary.exists() {
            return Ok(venv_binary);
        }
    }

    anyhow::bail!("Could not find karva_core binary TODO")
}

/// Print test output
fn print_test_output(
    printer: Printer,
    start_time: Instant,
    cache_dir: &Utf8PathBuf,
    run_hash: &RunHash,
    num_workers: usize,
    output_format: Option<&OutputFormat>,
) -> Result<AggregatedResults> {
    let reader = CacheReader::new(cache_dir, run_hash, num_workers)?;
    let result = reader.aggregate_results()?;

    let mut stdout = printer.stream_for_details().lock();

    let is_concise = matches!(output_format, Some(OutputFormat::Concise));

    if (!result.diagnostics.is_empty() || !result.discovery_diagnostics.is_empty())
        && result.stats.total() > 0
        && stdout.is_enabled()
    {
        writeln!(stdout)?;
    }

    if !result.discovery_diagnostics.is_empty() {
        writeln!(stdout, "discovery diagnostics:")?;
        writeln!(stdout)?;
        write!(stdout, "{}", result.discovery_diagnostics)?;

        if is_concise {
            writeln!(stdout)?;
        }
    }

    if !result.diagnostics.is_empty() {
        writeln!(stdout, "diagnostics:")?;
        writeln!(stdout)?;
        write!(stdout, "{}", result.diagnostics)?;

        if is_concise {
            writeln!(stdout)?;
        }
    }

    if (result.diagnostics.is_empty() && result.discovery_diagnostics.is_empty())
        && result.stats.total() > 0
        && stdout.is_enabled()
    {
        writeln!(stdout)?;
    }

    let mut result_stdout = printer.stream_for_failure_summary().lock();

    write!(result_stdout, "{}", result.stats.display(start_time))?;

    Ok(result)
}
