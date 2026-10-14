//! Worker selection registration and child-process construction.

use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result};
use karva_ipc::output_capture::OutputCaptureSink;
use karva_ipc::{ControllerServer, WorkerSelection};
use tempfile::NamedTempFile;

use karva_logging::Printer;

use crate::partition::Partition;
use crate::worker_args::{WorkerSpawn, worker_command};

use super::config::TestResultRetention;
use super::process_control;
use super::streams::{WorkerOutputForwarder, WorkerStderrForwarder};
use super::supervision::WorkerSupervisor;
use super::worker::WorkerResources;

/// Spawns one supervisor-owned child for each non-empty partition.
pub(super) fn spawn_workers(
    spawn: &WorkerSpawn,
    partitions: Vec<Partition>,
    controller: &mut ControllerServer,
    forward_stdout: bool,
    test_capacity: usize,
    result_retention: TestResultRetention,
    printer: Printer,
) -> Result<WorkerSupervisor> {
    let mut supervisor =
        WorkerSupervisor::with_test_capacity(test_capacity, result_retention, printer);

    for (worker_id, partition) in partitions.into_iter().enumerate() {
        if partition.is_empty() {
            tracing::debug!(target: "karva_runner::orchestration", "Skipping worker {} with no tests", worker_id);
            continue;
        }
        spawn_worker(
            &mut supervisor,
            spawn,
            controller,
            worker_id,
            partition,
            forward_stdout,
        )?;
    }

    Ok(supervisor)
}

/// Registers worker selection before spawning its process and stream forwarders.
pub(super) fn spawn_worker(
    supervisor: &mut WorkerSupervisor,
    spawn: &WorkerSpawn,
    controller: &mut ControllerServer,
    worker_id: usize,
    partition: Partition,
    forward_stdout: bool,
) -> Result<()> {
    let test_count = partition.test_count();
    let marker = fastrand::u128(..).to_ne_bytes();
    let limit = spawn
        .configuration
        .options
        .terminal
        .as_ref()
        .and_then(|terminal| terminal.max_output_bytes)
        .unwrap_or(karva_metadata::DEFAULT_MAX_OUTPUT_BYTES);
    let capture = Arc::new(OutputCaptureSink::new(marker, limit));
    controller.register_worker_selection(
        worker_id,
        WorkerSelection {
            output_marker: Some(marker),
            output_capture: Some(Arc::clone(&capture)),
            configuration: Arc::clone(&spawn.configuration),
            coverage_data_file: spawn
                .coverage_enabled
                .then(|| spawn.artifacts.coverage_data_file(worker_id)),
            test_paths: partition.worker_test_paths(),
            resume_skip: partition.resume_skip().to_vec(),
            resume_attempts: partition.resume_attempts().to_vec(),
        },
    )?;
    let stderr_capture = NamedTempFile::new().context("Failed to create worker stderr spool")?;
    let stderr_file = stderr_capture
        .reopen()
        .context("Failed to reopen worker stderr spool")?;
    let mut command = worker_command(spawn, worker_id);
    command.stderr(Stdio::piped());
    process_control::configure_worker_command(&mut command);
    command.stdout(Stdio::piped());

    let mut child =
        process_control::spawn(command).context("Failed to spawn karva-worker process")?;
    let output = process_control::take_stdout(&mut child)
        .map(|stdout| WorkerOutputForwarder::spawn(stdout, Arc::clone(&capture), forward_stdout));

    let stderr = process_control::take_stderr(&mut child)
        .map(|stderr| WorkerStderrForwarder::spawn(stderr, stderr_file, Arc::clone(&capture)))
        .context("Failed to capture karva-worker stderr")?;

    tracing::info!(target: "karva_runner::orchestration", "Worker {} spawned with {} tests", worker_id, test_count);
    supervisor.spawn(
        worker_id,
        partition,
        WorkerResources::new(child, output, stderr, stderr_capture, capture),
    );
    Ok(())
}
