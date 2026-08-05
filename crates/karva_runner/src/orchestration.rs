use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdout, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use camino::{Utf8Path, Utf8PathBuf};
use colored::Colorize;
use crossbeam_channel::{Receiver, TryRecvError};

use crate::shutdown::shutdown_receiver;
use karva_cache::{
    CACHE_DIR, RunArtifacts, RunHash, read_last_failed, read_recent_durations, write_durations,
    write_last_failed as persist_last_failed,
};
use karva_cli::{PartitionSelection, SubTestCommand};
use karva_collector::{CollectedPackage, CollectionSettings};
use karva_diagnostic::AggregatedResults;
use karva_ipc::{ControllerServer, WorkerEvent};
use karva_logging::Printer;
use karva_logging::time::{format_duration, format_duration_bracketed};
use karva_project::Project;
use karva_python_semantic::TestCacheKey;

use crate::binary::find_karva_worker_binary;
use crate::collection::ParallelCollector;
use crate::partition::{Partition, TestOrdering, partition_collected_tests, scheduled_test_count};
use crate::worker_args::{WorkerSpawn, worker_command};

/// Width that result labels (`PASS`, `FAIL`, `SIGINT`) are right-padded to so
/// columns align. Mirrors the constant in `karva_diagnostic::reporter`.
const LABEL_COLUMN_WIDTH: usize = 12;
/// Delay before cancellation snapshot so reader threads can publish current state.
const CURRENT_TEST_SETTLE: Duration = Duration::from_millis(50);

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
/// Child process plus controller-owned output forwarding and timing state.
struct Worker {
    id: usize,
    child: Child,
    output: Option<WorkerOutputForwarder>,
    start_time: Instant,
}

impl Worker {
    fn new(id: usize, child: Child, output: Option<WorkerOutputForwarder>) -> Self {
        Self {
            id,
            child,
            output,
            start_time: Instant::now(),
        }
    }

    fn duration(&self) -> Duration {
        self.start_time.elapsed()
    }

    fn join_output(&mut self) {
        if let Some(output) = self.output.take() {
            output.join(self.id);
        }
    }
}

#[derive(Default)]
/// Owns all live workers and guarantees they are reaped during shutdown.
struct WorkerManager {
    workers: Vec<Worker>,
    dispatcher: EventDispatcher,
}

/// Linearizes worker events into controller-owned run state.
///
/// Process supervision deliberately stays in [`WorkerManager`]. This mirrors
/// nextest's dispatcher/executor boundary and leaves reporting or recording
/// consumers independent of the transport and child-process lifecycle.
#[derive(Default)]
struct EventDispatcher {
    expected_workers: HashSet<usize>,
    completed_workers: HashSet<usize>,
    in_flight: HashMap<usize, RunningTest>,
    results: AggregatedResults,
}

/// Worker-reported snapshot of its current test.
#[derive(Debug)]
struct RunningTest {
    name: Option<String>,
    elapsed: Duration,
}

#[derive(Debug)]
/// Background thread preserving worker stdout order without blocking orchestration.
struct WorkerOutputForwarder {
    handle: JoinHandle<std::io::Result<()>>,
}

impl WorkerOutputForwarder {
    fn spawn(stdout: ChildStdout) -> Self {
        let handle = thread::spawn(move || forward_worker_stdout(stdout));
        Self { handle }
    }

    fn join(self, worker_id: usize) {
        match self.handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(err)) if err.kind() == std::io::ErrorKind::BrokenPipe => {}
            Ok(Err(err)) => tracing::warn!(worker_id, "failed to forward worker stdout: {err}"),
            Err(err) => tracing::warn!(worker_id, ?err, "worker stdout forwarder panicked"),
        }
    }
}

fn forward_worker_stdout(stdout: ChildStdout) -> std::io::Result<()> {
    let mut reader = BufReader::new(stdout);
    let mut line = Vec::new();

    loop {
        line.clear();
        let bytes_read = reader.read_until(b'\n', &mut line)?;
        if bytes_read == 0 {
            return Ok(());
        }

        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&line)?;
    }
}

/// Snapshot of one worker's current test taken before process termination.
struct InFlightTest {
    worker_id: usize,
    name: Option<String>,
    elapsed: Duration,
}

/// Executing test converted into a synthetic failed result after interruption.
struct InterruptedTest {
    name: String,
    duration: Duration,
}

impl EventDispatcher {
    fn register_worker(&mut self, worker_id: usize) {
        self.expected_workers.insert(worker_id);
    }

    /// Applies every queued worker event and reports whether fail-fast fired.
    fn dispatch_pending(&mut self, server: &mut ControllerServer) -> Result<bool> {
        server.accept_pending()?;
        let mut fail_fast = false;
        while let Some(message) = server.try_recv()? {
            let worker_id = message.worker_id;
            if !self.expected_workers.contains(&worker_id) {
                anyhow::bail!("unknown Karva worker {worker_id} sent a controller event");
            }
            match message.event {
                WorkerEvent::CurrentTest { name, elapsed } => {
                    self.in_flight
                        .insert(worker_id, RunningTest { name, elapsed });
                }
                WorkerEvent::FailFast => fail_fast = true,
                WorkerEvent::Completed(results) => {
                    if !self.completed_workers.insert(worker_id) {
                        anyhow::bail!("Karva worker {worker_id} sent duplicate results");
                    }
                    self.in_flight.remove(&worker_id);
                    self.results.merge_worker(results);
                }
            }
        }
        Ok(fail_fast)
    }

    fn finish(&mut self, server: &mut ControllerServer) -> Result<()> {
        server.finish()?;
        self.dispatch_pending(server)?;
        Ok(())
    }
}

impl WorkerManager {
    fn spawn(&mut self, worker_id: usize, child: Child, output: Option<WorkerOutputForwarder>) {
        self.dispatcher.register_worker(worker_id);
        self.workers.push(Worker::new(worker_id, child, output));
    }

    fn reap_finished(&mut self, log_completion: bool) {
        self.workers
            .retain_mut(|worker| match worker.child.try_wait() {
                Ok(Some(status)) => {
                    worker.join_output();
                    if log_completion {
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
                    }
                    false
                }
                Ok(None) => true,
                Err(e) => {
                    tracing::error!("Error waiting on worker {}: {}", worker.id, e);
                    false
                }
            });
    }

    /// Wait for all workers to complete.
    ///
    /// Returns early if a message is received on `shutdown_rx`, a worker sends
    /// a fail-fast event, or `deadline` passes. Finished workers are reaped at the
    /// top of each iteration before any of those conditions are checked, so a
    /// run that completes just as the deadline passes (or a signal arrives) is
    /// reported as `AllCompleted` rather than `TimedOut`/`Cancelled`.
    ///
    /// `deadline` is the absolute instant at which the whole run times out; it
    /// is computed before collection so the limit covers the entire run.
    fn wait_for_completion(
        &mut self,
        shutdown_rx: Option<&Receiver<()>>,
        server: &mut ControllerServer,
        fail_fast_enabled: bool,
        deadline: Option<Instant>,
    ) -> Result<WaitOutcome> {
        if self.workers.is_empty() {
            return Ok(WaitOutcome::AllCompleted);
        }

        tracing::info!(
            "Waiting for {} workers to complete (Ctrl+C to cancel)",
            self.workers.len()
        );

        loop {
            let fail_fast = self.dispatcher.dispatch_pending(server)?;
            self.reap_finished(true);

            if self.workers.is_empty() {
                self.dispatcher.finish(server)?;
                if self.dispatcher.completed_workers != self.dispatcher.expected_workers {
                    let mut missing: Vec<_> = self
                        .dispatcher
                        .expected_workers
                        .difference(&self.dispatcher.completed_workers)
                        .copied()
                        .collect();
                    missing.sort_unstable();
                    anyhow::bail!("Karva workers {missing:?} exited without sending results");
                }
                tracing::info!("All workers completed");
                return Ok(WaitOutcome::AllCompleted);
            }

            if let Some(rx) = shutdown_rx {
                match rx.try_recv() {
                    Ok(()) | Err(TryRecvError::Disconnected) => {
                        tracing::info!("Shutdown requested — stopping remaining workers");
                        return Ok(WaitOutcome::Cancelled);
                    }
                    Err(TryRecvError::Empty) => {}
                }
            }

            if fail_fast_enabled && fail_fast {
                tracing::info!("Fail-fast signal received — stopping remaining workers");
                return Ok(WaitOutcome::FailFast);
            }

            if let Some(deadline) = deadline
                && Instant::now() >= deadline
            {
                tracing::info!("Run timeout exceeded — stopping remaining workers");
                return Ok(WaitOutcome::TimedOut);
            }

            std::thread::sleep(WORKER_POLL_INTERVAL);
        }
    }

    /// Terminate and wait on any remaining worker processes.
    ///
    /// Uses separate phases: send graceful termination to all workers, wait
    /// for the configured grace period, then force-kill anything that remains.
    fn terminate_remaining(&mut self, grace_period: Duration) {
        if self.workers.is_empty() {
            return;
        }

        let processes: Vec<_> = self
            .workers
            .iter()
            .filter(|worker| !self.dispatcher.completed_workers.contains(&worker.id))
            .map(|worker| WorkerProcess {
                worker_id: worker.id,
                process_id: worker.child.id(),
            })
            .collect();

        for worker in &mut self.workers {
            if self.dispatcher.completed_workers.contains(&worker.id) {
                continue;
            }
            #[cfg(unix)]
            let terminate_result = process_control::terminate(&worker.child);
            #[cfg(not(unix))]
            let terminate_result = process_control::terminate(&mut worker.child);
            if let Err(err) = terminate_result {
                tracing::warn!(
                    worker_id = worker.id,
                    "failed to terminate worker process: {err}"
                );
            }
        }

        let deadline = Instant::now() + grace_period;
        loop {
            self.reap_finished(false);
            if self.workers.is_empty()
                && !processes
                    .iter()
                    .any(|process| process_control::is_running(process.process_id))
            {
                return;
            }
            if grace_period.is_zero() || Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(WORKER_POLL_INTERVAL);
        }

        for process in &processes {
            if let Err(err) = process_control::force_kill(process.process_id) {
                tracing::warn!(
                    worker_id = process.worker_id,
                    "failed to force-kill worker process group: {err}"
                );
            }
        }

        for worker in &mut self.workers {
            #[cfg(not(unix))]
            if let Err(err) = process_control::force_kill_child(&mut worker.child) {
                tracing::warn!(
                    worker_id = worker.id,
                    "failed to force-kill worker process: {err}"
                );
            }
            if let Err(err) = worker.child.wait() {
                tracing::warn!(
                    worker_id = worker.id,
                    "failed to wait for worker process: {err}"
                );
            }
            worker.join_output();
        }
        self.workers.clear();
    }

    /// Stop remaining workers and emit nextest-style cancellation lines.
    ///
    /// Each worker keeps current state in memory and answers one query here.
    /// We snapshot that state before killing and remember a
    /// `(worker_id, test name, elapsed time)` record for each.
    ///
    /// Workers are killed, reaped, and have their forwarded stdout drained
    /// before we print so any in-flight `PASS`/`FAIL` lines land before the
    /// cancellation block.
    fn cancel_and_kill(
        &mut self,
        printer: Printer,
        server: &mut ControllerServer,
        grace_period: Duration,
    ) -> Result<Vec<InterruptedTest>> {
        if self.workers.is_empty() {
            return Ok(Vec::new());
        }

        self.dispatcher.dispatch_pending(server)?;
        server.request_current_tests(self.workers.iter().map(|worker| worker.id));
        std::thread::sleep(CURRENT_TEST_SETTLE);
        self.dispatcher.dispatch_pending(server)?;

        let in_flight: Vec<_> = self
            .workers
            .iter()
            .map(|worker| {
                let current = self.dispatcher.in_flight.get(&worker.id);
                InFlightTest {
                    worker_id: worker.id,
                    name: current.and_then(|current| current.name.clone()),
                    elapsed: current.map_or(Duration::ZERO, |current| current.elapsed),
                }
            })
            .collect();

        let running_tests = in_flight.iter().filter(|test| test.name.is_some()).count();
        let test_label = if running_tests == 1 { "test" } else { "tests" };

        self.terminate_remaining(grace_period);

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

        Ok(in_flight
            .into_iter()
            .filter_map(|test| {
                test.name.map(|name| InterruptedTest {
                    name,
                    duration: test.elapsed,
                })
            })
            .collect())
    }
}

#[derive(Debug, Clone, Copy)]
struct WorkerProcess {
    worker_id: usize,
    process_id: u32,
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
}

/// Spawn worker processes for each partition
///
/// Creates a worker process for each non-empty partition, passing the appropriate
/// subset of tests and command-line arguments to each worker.
fn spawn_workers(
    spawn: &WorkerSpawn,
    partitions: &[Partition],
    forward_stdout: bool,
) -> Result<WorkerManager> {
    let mut worker_manager = WorkerManager::default();

    for (worker_id, partition) in partitions.iter().enumerate() {
        if partition.tests().is_empty() {
            tracing::debug!("Skipping worker {} with no tests", worker_id);
            continue;
        }

        let mut command = worker_command(spawn, worker_id, partition);
        command.stderr(Stdio::inherit());
        process_control::configure_worker_command(&mut command);
        if forward_stdout {
            command.stdout(Stdio::piped());
        } else {
            command.stdout(Stdio::inherit());
        }

        let mut child = command
            .spawn()
            .context("Failed to spawn karva-worker process")?;
        let output = if forward_stdout {
            child.stdout.take().map(WorkerOutputForwarder::spawn)
        } else {
            None
        };

        tracing::info!(
            "Worker {} spawned with {} tests",
            worker_id,
            partition.tests().len()
        );

        worker_manager.spawn(worker_id, child, output);
    }

    Ok(worker_manager)
}

/// Collect tests from the project without executing them.
pub fn collect_tests(project: &Project) -> Result<CollectedPackage> {
    let mut test_paths = Vec::new();

    for path in project.test_paths() {
        test_paths.push(path?);
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
    /// `karva_coverage::combine_and_report` to render the coverage table at
    /// the right point in its output sequence (after the test summary).
    pub coverage_files: Vec<Utf8PathBuf>,

    /// Whether the run was stopped because the configured run timeout elapsed.
    pub timed_out: bool,
}

/// Collects, partitions, executes, and aggregates one controller-side test run.
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
        tracing::info!(
            total_tests,
            requested_workers = config.num_workers,
            capped_workers = num_workers,
            "Capped worker count to avoid underutilized workers"
        );
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
    let scheduled_cases: usize = partitions
        .iter()
        .map(|partition| partition.tests().len())
        .sum();
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
        .filter(|partition| !partition.tests().is_empty())
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
            tracing::warn!("failed to write test start line: {err}");
        }
    }

    let run_hash = RunHash::current_time();
    let artifacts = RunArtifacts::new(&cache_dir, &run_hash);
    let mut controller = ControllerServer::bind(&run_hash.inner())?;

    tracing::info!("Spawning {} workers", scheduled_workers);

    let worker_binary = find_karva_worker_binary(project.cwd())?;
    let spawn = WorkerSpawn {
        project,
        artifacts: &artifacts,
        controller_address: controller.address()?,
        run_hash: &run_hash,
        args,
        num_workers,
        profile: config.profile.as_deref().unwrap_or("default"),
        worker_binary: &worker_binary,
        coverage_enabled: !project.settings().coverage().sources.is_empty(),
    };
    let forward_stdout = printer.stream_for_test_result().is_enabled();
    let mut worker_manager = spawn_workers(&spawn, &partitions, forward_stdout)?;

    let outcome = worker_manager.wait_for_completion(
        shutdown_rx,
        &mut controller,
        project.settings().max_fail().has_limit(),
        run_deadline,
    )?;
    let termination_grace_period = project.settings().test().termination_grace_period();
    let interrupted_tests = if outcome == WaitOutcome::Cancelled {
        worker_manager.cancel_and_kill(printer, &mut controller, termination_grace_period)?
    } else {
        worker_manager.terminate_remaining(termination_grace_period);
        Vec::new()
    };

    let timed_out = outcome == WaitOutcome::TimedOut;

    worker_manager.dispatcher.finish(&mut controller)?;
    let mut results = std::mem::take(&mut worker_manager.dispatcher.results);
    for test in interrupted_tests {
        results.register_interrupted_test(&test.name, test.duration);
    }

    if !config.no_cache {
        write_last_failed(&cache_dir, &results.failed_tests);
        if let Err(err) = write_durations(&cache_dir, &results.durations) {
            tracing::warn!("Failed to write test durations to cache: {err}");
        }
    }

    let coverage_files = if project.settings().coverage().sources.is_empty() {
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
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(10);
fn previous_durations(cache_dir: &Utf8Path, no_cache: bool) -> HashMap<TestCacheKey, Duration> {
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

fn last_failed_set(cache_dir: &Utf8Path, enabled: bool) -> HashSet<TestCacheKey> {
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

fn write_last_failed(cache_dir: &Utf8Path, failed_tests: &BTreeSet<TestCacheKey>) {
    let failed_tests = failed_tests.iter().cloned().collect::<Vec<_>>();
    if let Err(err) = persist_last_failed(cache_dir, &failed_tests) {
        tracing::warn!("Failed to write last-failed cache: {err}");
    }
}

#[cfg(unix)]
mod process_control {
    use std::io;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command};

    pub(super) fn configure_worker_command(command: &mut Command) {
        command.process_group(0);
    }

    pub(super) fn terminate(child: &Child) -> io::Result<()> {
        signal_process_group(child.id(), libc::SIGTERM)
    }

    pub(super) fn force_kill(process_id: u32) -> io::Result<()> {
        signal_process_group(process_id, libc::SIGKILL)
    }

    pub(super) fn is_running(process_id: u32) -> bool {
        let Ok(process_group_id) = process_group_id(process_id) else {
            return false;
        };
        #[expect(
            unsafe_code,
            reason = "checking Unix process groups requires libc::kill"
        )]
        let result = unsafe { libc::kill(-process_group_id, 0) };
        result == 0
    }

    fn signal_process_group(process_id: u32, signal: libc::c_int) -> io::Result<()> {
        let process_group_id = process_group_id(process_id)?;
        #[expect(
            unsafe_code,
            reason = "signalling Unix process groups requires libc::kill"
        )]
        let result = unsafe { libc::kill(-process_group_id, signal) };
        if result == 0 {
            return Ok(());
        }

        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::ESRCH) {
            Ok(())
        } else {
            Err(err)
        }
    }

    fn process_group_id(process_id: u32) -> io::Result<libc::pid_t> {
        libc::pid_t::try_from(process_id).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("process id {process_id} cannot be represented as pid_t"),
            )
        })
    }
}

#[cfg(not(unix))]
mod process_control {
    use std::io;
    use std::process::{Child, Command};

    pub(super) fn configure_worker_command(_command: &mut Command) {}

    pub(super) fn terminate(child: &mut Child) -> io::Result<()> {
        child.kill()
    }

    pub(super) fn force_kill(_process_id: u32) -> io::Result<()> {
        Ok(())
    }

    pub(super) fn force_kill_child(child: &mut Child) -> io::Result<()> {
        child.kill()
    }

    pub(super) fn is_running(_process_id: u32) -> bool {
        false
    }
}
