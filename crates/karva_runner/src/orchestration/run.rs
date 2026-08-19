//! Top-level phases of one parallel test run.

use std::collections::HashSet;
use std::io::Write;
use std::time::Instant;

use anyhow::Result;
use colored::Colorize;
use karva_cache::{CACHE_DIR, RunArtifacts, RunHash, write_durations};
use karva_cli::SubTestCommand;
use karva_ipc::ControllerServer;
use karva_logging::Printer;
use karva_project::Project;

use super::config::{DurationRetention, ParallelTestConfig, RunOutput};
use super::planning::{collect_tests, last_failed_set, previous_durations, write_last_failed};
use super::recovery::recover_crashed_workers;
use super::spawn::{spawn_worker, spawn_workers};
use super::supervision::WaitOutcome;
use crate::binary::find_karva_worker_binary;
use crate::partition::{Partition, partition_collected_tests, scheduled_test_count};
use crate::shutdown::shutdown_receiver;
use crate::worker_args::WorkerSpawn;

/// Collects, partitions, executes, and aggregates one controller-side test run.
pub fn run_parallel_tests(
    project: &Project,
    config: &ParallelTestConfig,
    args: &SubTestCommand,
    printer: Printer,
) -> Result<RunOutput> {
    let retain_durations = matches!(config.duration_retention, DurationRetention::Retain);
    // Install the Ctrl+C handler before any potentially long-running work
    // (collection, partitioning, worker spawn). Otherwise an early SIGINT
    // hits the default disposition and the run terminates silently with no
    // cancellation banner.
    let shutdown_rx = if config.create_ctrlc_handler {
        Some(shutdown_receiver())
    } else {
        None
    };

    // Anchor the run-timeout deadline before collection so the limit covers
    // the whole run, not just test execution.
    let run_deadline = project
        .settings()
        .test()
        .run_timeout
        .map(|timeout| Instant::now() + timeout);

    let collected = collect_tests(project)?;

    let total_tests = scheduled_test_count(&collected);
    let max_useful_workers = total_tests.div_ceil(MIN_TESTS_PER_WORKER).max(1);
    let num_workers = config.num_workers.min(max_useful_workers);

    if num_workers < config.num_workers {
        tracing::info!(target: "karva_runner::orchestration",
            total_tests,
            requested_workers = config.num_workers,
            capped_workers = num_workers,
            "Capped worker count to avoid underutilized workers"
        );
    }

    tracing::debug!(target: "karva_runner::orchestration", num_workers, "Partitioning tests");

    let cache_dir = project.cwd().join(CACHE_DIR);

    let previous_durations = previous_durations(&cache_dir, config.no_cache);

    if !previous_durations.is_empty() {
        tracing::debug!(target: "karva_runner::orchestration",
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
    let scheduled_cases: usize = partitions.iter().map(Partition::test_count).sum();
    let scheduled_tests = if config.last_failed || config.partition.is_some() {
        partitions
            .iter()
            .flat_map(Partition::function_roots)
            .collect::<HashSet<_>>()
            .len()
    } else {
        collected.test_count()
    };
    let scheduled_workers = partitions
        .iter()
        .filter(|partition| !partition.is_empty())
        .count();

    if scheduled_cases > 0 {
        let mut stdout = printer.stream_for_test_result().lock();
        let label = format!("{:>12}", "Starting").green().bold();
        let test_label = if scheduled_tests == 1 {
            "test"
        } else {
            "tests"
        };
        let worker_label = if scheduled_workers == 1 {
            "worker"
        } else {
            "workers"
        };
        let total_tests_bold = scheduled_tests.to_string().bold();
        let num_workers_bold = scheduled_workers.to_string().bold();
        if let Err(err) = writeln!(
            stdout,
            "{label} {total_tests_bold} {test_label} across {num_workers_bold} {worker_label}"
        ) {
            tracing::warn!(target: "karva_runner::orchestration", "failed to write test start line: {err}");
        }
    }

    let run_hash = RunHash::current_time();
    let artifacts = RunArtifacts::new(&cache_dir, &run_hash);
    let mut controller = ControllerServer::bind(&run_hash.inner())?;

    tracing::info!(target: "karva_runner::orchestration", "Spawning {} workers", scheduled_workers);

    let worker_binary = find_karva_worker_binary(project.cwd())?;
    let spawn = WorkerSpawn {
        project,
        artifacts: &artifacts,
        controller_endpoint: controller.endpoint(),
        run_hash: &run_hash,
        args,
        num_workers,
        profile: config.profile.as_deref().unwrap_or("default"),
        worker_binary: &worker_binary,
        coverage_enabled: !project.settings().coverage().sources.is_empty(),
    };
    let forward_stdout = printer.stream_for_test_result().is_enabled();
    let mut next_worker_id = partitions.len();
    let mut worker_crashed = false;
    let mut worker_manager = spawn_workers(
        &spawn,
        partitions,
        &mut controller,
        forward_stdout,
        scheduled_cases,
        config.result_retention,
        retain_durations,
    )?;

    let max_fail = project.settings().max_fail();
    let outcome = loop {
        match worker_manager.wait_for_completion(
            shutdown_rx,
            &mut controller,
            max_fail,
            run_deadline,
        )? {
            WaitOutcome::WorkersCrashed(crashed_workers) => {
                worker_crashed = true;
                let recovery = recover_crashed_workers(
                    &mut worker_manager,
                    &mut controller,
                    crashed_workers,
                    max_fail,
                    printer,
                )?;
                if recovery.failure_limit_reached() {
                    tracing::info!(target: "karva_runner::orchestration", "Failure budget exhausted — stopping remaining workers");
                    break WaitOutcome::FailFast;
                }
                for pending in recovery.into_replacements() {
                    spawn_worker(
                        &mut worker_manager,
                        &spawn,
                        &mut controller,
                        next_worker_id,
                        pending,
                        forward_stdout,
                    )?;
                    next_worker_id += 1;
                }
                if !worker_manager.has_workers() {
                    break WaitOutcome::AllCompleted;
                }
            }
            outcome => break outcome,
        }
    };
    let termination_grace_period = project.settings().test().termination_grace_period();
    let interrupted_tests = if matches!(outcome, WaitOutcome::Cancelled) {
        worker_manager.cancel_and_kill(printer, &mut controller, termination_grace_period)?
    } else {
        worker_manager.terminate_remaining(termination_grace_period);
        if matches!(outcome, WaitOutcome::FailFast | WaitOutcome::TimedOut) {
            controller.disconnect_readers()?;
        }
        Vec::new()
    };

    let timed_out = matches!(outcome, WaitOutcome::TimedOut);

    worker_manager.finish_events(&mut controller)?;
    let mut results = worker_manager.take_results();
    for test in interrupted_tests {
        results.register_interrupted_test(&test.name, test.duration, retain_durations);
    }
    let results = results.into_sorted();

    if !config.no_cache {
        write_last_failed(&cache_dir, &results.failed_tests);
        if let Err(err) = write_durations(&cache_dir, &results.durations) {
            tracing::warn!(target: "karva_runner::orchestration", "Failed to write test durations to cache: {err}");
        }
    }

    let coverage_files = if project.settings().coverage().sources.is_empty() || worker_crashed {
        if worker_crashed && !project.settings().coverage().sources.is_empty() {
            tracing::warn!(target: "karva_runner::orchestration",
                "Coverage report skipped because a crashed worker could not save complete data"
            );
        }
        Vec::new()
    } else {
        artifacts.coverage_files()?
    };

    Ok(RunOutput {
        results,
        coverage_files,
        timed_out,
    })
}

const MIN_TESTS_PER_WORKER: usize = 5;
