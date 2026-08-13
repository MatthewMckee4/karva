//! Worker selection registration and child-process construction.

use std::process::Stdio;

use anyhow::{Context, Result};
use karva_ipc::{ControllerServer, WorkerSelection};
use tempfile::NamedTempFile;

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
) -> Result<WorkerSupervisor> {
    let mut supervisor = WorkerSupervisor::with_test_capacity(test_capacity, result_retention);

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
    controller.register_worker_selection(
        worker_id,
        WorkerSelection {
            test_paths: partition.worker_test_paths(),
            resume_skip: partition.resume_skip().to_vec(),
        },
    )?;
    let stderr_capture = NamedTempFile::new().context("Failed to create worker stderr spool")?;
    let stderr_file = stderr_capture
        .reopen()
        .context("Failed to reopen worker stderr spool")?;
    let mut command = worker_command(spawn, worker_id);
    command.stderr(Stdio::piped());
    process_control::configure_worker_command(&mut command);
    command.stdout(if forward_stdout {
        Stdio::piped()
    } else {
        Stdio::inherit()
    });

    let mut child = command
        .spawn()
        .context("Failed to spawn karva-worker process")?;
    let output = if forward_stdout {
        child.stdout.take().map(WorkerOutputForwarder::spawn)
    } else {
        None
    };
    let stderr = child
        .stderr
        .take()
        .map(|stderr| WorkerStderrForwarder::spawn(stderr, stderr_file))
        .context("Failed to capture karva-worker stderr")?;

    tracing::info!(target: "karva_runner::orchestration", "Worker {} spawned with {} tests", worker_id, test_count);
    supervisor.spawn(
        worker_id,
        partition,
        WorkerResources::new(child, output, stderr, stderr_capture),
    );
    Ok(())
}
