use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, Write};
use std::process::{Child, ChildStderr, ChildStdout, ExitStatus, Stdio};
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
use karva_ipc::{ControllerServer, WorkerEvent, WorkerSelection};
use karva_logging::time::{format_duration, format_duration_bracketed};
use karva_logging::{Printer, StatusLevel};
use karva_metadata::MaxFail;
use karva_project::Project;
use karva_python_semantic::TestCacheKey;
use tempfile::NamedTempFile;

use crate::binary::find_karva_worker_binary;
use crate::collection::ParallelCollector;
use crate::partition::{Partition, TestOrdering, partition_collected_tests, scheduled_test_count};
use crate::worker_args::{WorkerSpawn, worker_command};

/// Width that result labels (`PASS`, `FAIL`, `SIGINT`) are right-padded to so
/// columns align. Mirrors the constant in `karva_diagnostic::reporter`.
const LABEL_COLUMN_WIDTH: usize = 12;
// Receipt: worker writes and controller reads each advance every 10 ms. With
// no window the cancellation integration test consistently missed the first
// TestStarted; five intervals passed 20 consecutive repetitions.
const CANCELLATION_EVENT_SETTLE: Duration = Duration::from_millis(50);

/// How `wait_for_completion` exited.
#[derive(Debug)]
enum WaitOutcome {
    /// Every worker exited on its own.
    AllCompleted,

    /// Ctrl+C was received; remaining workers must be killed.
    Cancelled,

    /// A worker hit the fail-fast budget; remaining workers must be killed.
    FailFast,

    /// The run timeout elapsed before the workers finished.
    TimedOut,

    /// One or more workers exited unexpectedly.
    WorkersCrashed(Vec<CrashedWorker>),
}

#[derive(Debug)]
/// Child process plus controller-owned output forwarding and timing state.
struct Worker {
    id: usize,
    child: Child,
    output: Option<WorkerOutputForwarder>,
    stderr: Option<WorkerStderrForwarder>,
    stderr_capture: NamedTempFile,
    partition: Partition,
    start_time: Instant,
    exit_status: Option<ExitStatus>,
    exit_observed: Option<Instant>,
    exit_event_count: usize,
    forced_disconnect: bool,
}

impl Worker {
    fn new(
        id: usize,
        child: Child,
        output: Option<WorkerOutputForwarder>,
        stderr: WorkerStderrForwarder,
        stderr_capture: NamedTempFile,
        partition: Partition,
    ) -> Self {
        Self {
            id,
            child,
            output,
            stderr: Some(stderr),
            stderr_capture,
            partition,
            start_time: Instant::now(),
            exit_status: None,
            exit_observed: None,
            exit_event_count: 0,
            forced_disconnect: false,
        }
    }

    fn duration(&self) -> Duration {
        self.start_time.elapsed()
    }

    fn join_output(&mut self) {
        if let Some(output) = self.output.take() {
            output.join(self.id, !self.forced_disconnect);
        }
    }

    fn join_stderr(&mut self, read: bool) -> String {
        if let Some(stderr) = self.stderr.take() {
            stderr.join(self.id, !self.forced_disconnect);
        }
        if !read {
            return String::new();
        }
        let file = self.stderr_capture.as_file_mut();
        match file.rewind().and_then(|()| {
            let mut output = Vec::new();
            file.read_to_end(&mut output)?;
            Ok(output)
        }) {
            Ok(output) => String::from_utf8_lossy(&output).into_owned(),
            Err(error) => {
                tracing::warn!(worker_id = self.id, "failed to read worker stderr: {error}");
                String::new()
            }
        }
    }
}

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
    completed_tests: HashSet<TestCacheKey>,
    in_flight: HashMap<usize, RunningTest>,
    results: AggregatedResults,
    result_retention: TestResultRetention,
}

impl WorkerManager {
    fn with_test_capacity(test_capacity: usize, result_retention: TestResultRetention) -> Self {
        let test_case_capacity = match result_retention {
            TestResultRetention::FailuresAndRetries => 0,
            TestResultRetention::All => test_capacity,
        };
        Self {
            workers: Vec::new(),
            dispatcher: EventDispatcher {
                expected_workers: HashSet::new(),
                completed_workers: HashSet::new(),
                completed_tests: HashSet::new(),
                in_flight: HashMap::new(),
                results: AggregatedResults::with_capacities(test_capacity, test_case_capacity),
                result_retention,
            },
        }
    }
}

/// Controller-owned state for one executing test.
#[derive(Debug)]
struct RunningTest {
    name: String,
    cache_key: TestCacheKey,
    started: Instant,
}

#[derive(Debug)]
/// Background thread preserving worker stdout order without blocking orchestration.
struct WorkerOutputForwarder {
    handle: JoinHandle<std::io::Result<()>>,
}

#[derive(Debug)]
struct WorkerStderrForwarder {
    handle: JoinHandle<std::io::Result<()>>,
}

impl WorkerStderrForwarder {
    fn spawn(stderr: ChildStderr, captured: File) -> Self {
        let handle = thread::spawn(move || forward_worker_stderr(stderr, captured));
        Self { handle }
    }

    fn join(self, worker_id: usize, wait: bool) {
        if !wait && !self.handle.is_finished() {
            return;
        }
        match self.handle.join() {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(worker_id, "failed to forward worker stderr: {error}");
            }
            Err(error) => {
                tracing::warn!(worker_id, ?error, "worker stderr forwarder panicked");
            }
        }
    }
}

impl WorkerOutputForwarder {
    fn spawn(stdout: ChildStdout) -> Self {
        let handle = thread::spawn(move || forward_worker_stdout(stdout));
        Self { handle }
    }

    fn join(self, worker_id: usize, wait: bool) {
        if !wait && !self.handle.is_finished() {
            return;
        }
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

fn forward_worker_stderr(stderr: ChildStderr, mut captured: File) -> std::io::Result<()> {
    let mut reader = BufReader::new(stderr);
    let mut capture_error = None;
    let mut forward_error = None;
    let mut last_byte = None;
    let mut forwarded = std::io::stderr().lock();
    loop {
        let bytes = reader.fill_buf()?;
        if bytes.is_empty() {
            break;
        }
        let consumed = bytes.len();
        last_byte = bytes.last().copied();
        if capture_error.is_none()
            && let Err(error) = captured.write_all(bytes)
        {
            capture_error = Some(error);
        }
        if forward_error.is_none()
            && let Err(error) = forwarded.write_all(bytes)
        {
            forward_error = Some(error);
        }
        reader.consume(consumed);
    }
    if last_byte.is_some_and(|byte| byte != b'\n')
        && forward_error.is_none()
        && let Err(error) = forwarded.write_all(b"\n")
    {
        forward_error = Some(error);
    }
    if capture_error.is_none()
        && let Err(error) = captured.flush()
    {
        capture_error = Some(error);
    }
    if let Some(error) = capture_error {
        Err(error)
    } else if let Some(error) = forward_error {
        if error.kind() == std::io::ErrorKind::BrokenPipe {
            Ok(())
        } else {
            Err(error)
        }
    } else {
        Ok(())
    }
}

#[derive(Debug)]
struct CrashedWorker {
    id: usize,
    partition: Partition,
    status: ExitStatus,
    stderr: String,
    active: Option<RunningTest>,
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

    /// Applies every queued worker event to controller-owned run state.
    fn dispatch_pending(&mut self, server: &mut ControllerServer) -> Result<()> {
        server.accept_pending()?;
        while let Some(message) = server.try_recv()? {
            let worker_id = message.worker_id;
            if !self.expected_workers.contains(&worker_id) {
                anyhow::bail!("unknown Karva worker {worker_id} sent a controller event");
            }
            match *message.event {
                WorkerEvent::TestStarted { name, cache_key } => {
                    if let Some(running) = self.in_flight.get_mut(&worker_id) {
                        if running.cache_key.test_function_name() != cache_key.test_function_name()
                        {
                            anyhow::bail!(
                                "Karva worker {worker_id} started `{name}` before finishing `{}`",
                                running.name
                            );
                        }
                        running.name = name;
                        running.cache_key = cache_key;
                    } else {
                        self.in_flight.insert(
                            worker_id,
                            RunningTest {
                                name,
                                cache_key,
                                started: Instant::now(),
                            },
                        );
                    }
                }
                WorkerEvent::TestSlow => self.results.register_slow_test(),
                WorkerEvent::TestFinished { cache_key, result } => {
                    if let Some(running) = self.in_flight.remove(&worker_id)
                        && running.cache_key != cache_key
                    {
                        anyhow::bail!(
                            "Karva worker {worker_id} started `{}` but finished `{}`",
                            running.name,
                            result.full_name()
                        );
                    }
                    self.completed_tests.insert(cache_key.clone());
                    self.results.register_rendered_test_case(
                        cache_key,
                        *result,
                        matches!(self.result_retention, TestResultRetention::All),
                    );
                }
                WorkerEvent::RunDiagnostic(diagnostic) => {
                    self.results.add_rendered_run_diagnostic(diagnostic);
                }
                WorkerEvent::WorkerFinished => {
                    if !self.completed_workers.insert(worker_id) {
                        anyhow::bail!("Karva worker {worker_id} completed more than once");
                    }
                    if let Some(running) = self.in_flight.get(&worker_id) {
                        anyhow::bail!(
                            "Karva worker {worker_id} completed while `{}` was still running",
                            running.name
                        );
                    }
                }
            }
        }
        Ok(())
    }

    fn finish(&mut self, server: &mut ControllerServer) -> Result<()> {
        server.finish()?;
        self.dispatch_pending(server)?;
        Ok(())
    }

    fn abandon_worker(&mut self, worker_id: usize) {
        self.expected_workers.remove(&worker_id);
        self.completed_workers.remove(&worker_id);
    }
}

impl WorkerManager {
    fn spawn(
        &mut self,
        worker_id: usize,
        child: Child,
        output: Option<WorkerOutputForwarder>,
        stderr: WorkerStderrForwarder,
        stderr_capture: NamedTempFile,
        partition: Partition,
    ) {
        self.dispatcher.register_worker(worker_id);
        self.workers.push(Worker::new(
            worker_id,
            child,
            output,
            stderr,
            stderr_capture,
            partition,
        ));
    }

    fn reap_finished(&mut self, server: &ControllerServer) -> Result<Vec<CrashedWorker>> {
        let mut running = Vec::new();
        let mut crashed = Vec::new();
        for mut worker in self.workers.drain(..) {
            let status = if let Some(status) = worker.exit_status {
                Ok(Some(status))
            } else {
                #[cfg(unix)]
                if process_control::has_exited(&worker.child)? {
                    if !server.worker_disconnected(worker.id)
                        && let Err(error) = process_control::force_kill(worker.child.id())
                        && error.kind() != std::io::ErrorKind::PermissionDenied
                    {
                        tracing::warn!(
                            worker_id = worker.id,
                            "failed to clean up worker process group: {error}"
                        );
                    }
                    worker.child.wait().map(Some)
                } else {
                    Ok(None)
                }
                #[cfg(not(unix))]
                worker.child.try_wait()
            };
            match status {
                Ok(Some(status)) => {
                    if worker.exit_status.is_none() {
                        worker.exit_status = Some(status);
                        worker.exit_observed = Some(Instant::now());
                        worker.exit_event_count = server.worker_event_count(worker.id);
                    }
                    if server.worker_started(worker.id)? && !server.worker_disconnected(worker.id) {
                        let event_count = server.worker_event_count(worker.id);
                        if event_count != worker.exit_event_count {
                            worker.exit_event_count = event_count;
                            worker.exit_observed = Some(Instant::now());
                        }
                        if worker
                            .exit_observed
                            .is_some_and(|observed| observed.elapsed() >= CANCELLATION_EVENT_SETTLE)
                        {
                            server.disconnect_worker(worker.id)?;
                            worker.forced_disconnect = true;
                        }
                        running.push(worker);
                        continue;
                    }
                    worker.join_output();
                    let completed =
                        status.success() && self.dispatcher.completed_workers.contains(&worker.id);
                    if completed {
                        worker.join_stderr(false);
                        tracing::info!(
                            "Worker {} completed successfully in {}",
                            worker.id,
                            format_duration(worker.duration()),
                        );
                    } else {
                        let duration = worker.duration();
                        let stderr = worker.join_stderr(true);
                        tracing::error!(
                            "Worker {} failed with {} in {}",
                            worker.id,
                            termination_description(status),
                            format_duration(duration),
                        );
                        let active = self.dispatcher.in_flight.remove(&worker.id);
                        crashed.push(CrashedWorker {
                            id: worker.id,
                            partition: worker.partition,
                            status,
                            stderr,
                            active,
                        });
                    }
                }
                Ok(None) => running.push(worker),
                Err(error) => {
                    tracing::error!("Error waiting on worker {}: {}", worker.id, error);
                }
            }
        }
        self.workers = running;
        Ok(crashed)
    }

    fn reap_during_shutdown(&mut self) {
        self.workers
            .retain_mut(|worker| match worker.child.try_wait() {
                Ok(Some(_)) => {
                    worker.join_output();
                    worker.join_stderr(false);
                    false
                }
                Ok(None) => true,
                Err(error) => {
                    tracing::error!("Error waiting on worker {}: {}", worker.id, error);
                    false
                }
            });
    }

    /// Wait for all workers to complete.
    ///
    /// Returns early if a message is received on `shutdown_rx`, the global
    /// failure budget is exhausted, or `deadline` passes. Finished workers are reaped at the
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
        max_fail: MaxFail,
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
            self.dispatcher.dispatch_pending(server)?;
            let crashed = self.reap_finished(server)?;
            if !crashed.is_empty() {
                return Ok(WaitOutcome::WorkersCrashed(crashed));
            }

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

            let failures =
                self.dispatcher.results.stats().failed() + self.dispatcher.results.stats().errors();
            let failures = u32::try_from(failures).unwrap_or(u32::MAX);
            if max_fail.is_exceeded_by(failures) {
                tracing::info!("Failure budget exhausted — stopping remaining workers");
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
            self.reap_during_shutdown();
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
            worker.join_stderr(false);
        }
        self.workers.clear();
    }

    /// Stop remaining workers and emit nextest-style cancellation lines.
    ///
    /// Controller lifecycle events identify tests still in flight after every
    /// worker has stopped and its event stream has drained.
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
        std::thread::sleep(CANCELLATION_EVENT_SETTLE);
        self.dispatcher.dispatch_pending(server)?;
        let mut worker_ids = Vec::with_capacity(self.workers.len());
        for worker in &self.workers {
            worker_ids.push(worker.id);
        }
        self.terminate_remaining(grace_period);
        self.dispatcher.finish(server)?;

        let in_flight: Vec<_> = worker_ids
            .into_iter()
            .map(|worker_id| {
                let current = self.dispatcher.in_flight.get(&worker_id);
                InFlightTest {
                    worker_id,
                    name: current.map(|current| current.name.clone()),
                    elapsed: current.map_or(Duration::ZERO, |current| current.started.elapsed()),
                }
            })
            .collect();

        let running_tests = in_flight.iter().filter(|test| test.name.is_some()).count();
        let test_label = if running_tests == 1 { "test" } else { "tests" };

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

impl Drop for WorkerManager {
    fn drop(&mut self) {
        for worker in &mut self.workers {
            if worker.exit_status.is_none()
                && let Err(error) = process_control::force_kill(worker.child.id())
            {
                tracing::warn!(
                    worker_id = worker.id,
                    "failed to clean up worker process group: {error}"
                );
            }
            #[cfg(not(unix))]
            if let Err(error) = process_control::force_kill_child(&mut worker.child) {
                tracing::warn!(worker_id = worker.id, "failed to kill worker: {error}");
            }
            if let Err(error) = worker.child.wait() {
                tracing::warn!(worker_id = worker.id, "failed to reap worker: {error}");
            }
        }
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

fn print_crashed_test(printer: Printer, name: &str, duration: Duration) {
    if printer.status_level() == StatusLevel::None {
        return;
    }
    let label = "CRASH".red().bold();
    let padding = " ".repeat(LABEL_COLUMN_WIDTH.saturating_sub("CRASH".len()));
    let duration = format_duration_bracketed(duration);
    let name = format_in_flight_test(name);
    let mut stdout = printer.stream_for_test_result().lock();
    if let Err(error) = writeln!(stdout, "{padding}{label} {duration} {name}") {
        tracing::warn!("failed to write crashed test line: {error}");
    }
}

fn termination_description(status: ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;

        if let Some(signal) = status.signal() {
            let name = match signal {
                libc::SIGABRT => "SIGABRT",
                libc::SIGBUS => "SIGBUS",
                libc::SIGILL => "SIGILL",
                libc::SIGSEGV => "SIGSEGV",
                libc::SIGTRAP => "SIGTRAP",
                _ => "signal",
            };
            return format!("{name} ({signal})");
        }
    }
    status.code().map_or_else(
        || "an unknown status".to_string(),
        |code| format!("exit code {code}"),
    )
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

    /// Which completed test case bodies the controller retains.
    pub result_retention: TestResultRetention,
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

/// Spawn worker processes for each partition
///
/// Creates a worker process for each non-empty partition, registering its
/// owned test selection with the IPC controller before spawn.
fn spawn_workers(
    spawn: &WorkerSpawn,
    partitions: Vec<Partition>,
    controller: &mut ControllerServer,
    forward_stdout: bool,
    test_capacity: usize,
    result_retention: TestResultRetention,
) -> Result<WorkerManager> {
    let mut worker_manager = WorkerManager::with_test_capacity(test_capacity, result_retention);

    for (worker_id, partition) in partitions.into_iter().enumerate() {
        if partition.tests().is_empty() {
            tracing::debug!("Skipping worker {} with no tests", worker_id);
            continue;
        }

        spawn_worker(
            &mut worker_manager,
            spawn,
            controller,
            worker_id,
            partition,
            forward_stdout,
        )?;
    }

    Ok(worker_manager)
}

fn spawn_worker(
    worker_manager: &mut WorkerManager,
    spawn: &WorkerSpawn,
    controller: &mut ControllerServer,
    worker_id: usize,
    partition: Partition,
    forward_stdout: bool,
) -> Result<()> {
    let test_count = partition.tests().len();
    controller.register_worker_selection(
        worker_id,
        WorkerSelection {
            test_paths: partition.tests().to_vec(),
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
    let stderr = child
        .stderr
        .take()
        .map(|stderr| WorkerStderrForwarder::spawn(stderr, stderr_file))
        .context("Failed to capture karva-worker stderr")?;

    tracing::info!("Worker {} spawned with {} tests", worker_id, test_count);
    worker_manager.spawn(worker_id, child, output, stderr, stderr_capture, partition);
    Ok(())
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
        collect_doctests: project.settings().test().doctest_modules,
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
    let mut next_worker_id = partitions.len();
    let mut worker_crashed = false;
    let mut worker_manager = spawn_workers(
        &spawn,
        partitions,
        &mut controller,
        forward_stdout,
        scheduled_cases,
        config.result_retention,
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
                let mut replacements = Vec::new();
                for crashed_worker in crashed_workers {
                    worker_manager.dispatcher.abandon_worker(crashed_worker.id);
                    let termination = termination_description(crashed_worker.status);
                    let Some(active) = crashed_worker.active else {
                        worker_manager.dispatcher.results.register_worker_exit(
                            crashed_worker.id,
                            &termination,
                            &crashed_worker.stderr,
                        );
                        let pending = crashed_worker
                            .partition
                            .pending_after_crash(&worker_manager.dispatcher.completed_tests, None);
                        if !pending.tests().is_empty() {
                            replacements.push(pending);
                        }
                        continue;
                    };
                    let active = {
                        let duration = active.started.elapsed();
                        (active.name, active.cache_key, duration)
                    };
                    let (name, cache_key, duration) = active;
                    print_crashed_test(printer, &name, duration);
                    let pending = crashed_worker.partition.pending_after_crash(
                        &worker_manager.dispatcher.completed_tests,
                        Some(&cache_key),
                    );
                    worker_manager.dispatcher.results.register_crashed_test(
                        &name,
                        cache_key,
                        duration,
                        &termination,
                        &crashed_worker.stderr,
                    );
                    if !pending.tests().is_empty() {
                        replacements.push(pending);
                    }
                }
                let failures = worker_manager.dispatcher.results.stats().failed()
                    + worker_manager.dispatcher.results.stats().errors();
                let failures = u32::try_from(failures).unwrap_or(u32::MAX);
                if max_fail.is_exceeded_by(failures) {
                    tracing::info!("Failure budget exhausted — stopping remaining workers");
                    break WaitOutcome::FailFast;
                }
                for pending in replacements {
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
                if worker_manager.workers.is_empty() {
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
        Vec::new()
    };

    let timed_out = matches!(outcome, WaitOutcome::TimedOut);

    worker_manager.dispatcher.finish(&mut controller)?;
    let mut results = std::mem::take(&mut worker_manager.dispatcher.results);
    for test in interrupted_tests {
        results.register_interrupted_test(&test.name, test.duration);
    }
    let results = results.into_sorted();

    if !config.no_cache {
        write_last_failed(&cache_dir, &results.failed_tests);
        if let Err(err) = write_durations(&cache_dir, &results.durations) {
            tracing::warn!("Failed to write test durations to cache: {err}");
        }
    }

    let coverage_files = if project.settings().coverage().sources.is_empty() || worker_crashed {
        if worker_crashed && !project.settings().coverage().sources.is_empty() {
            tracing::warn!(
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

    /// Observes worker exit without reaping its process-group leader.
    pub(super) fn has_exited(child: &Child) -> io::Result<bool> {
        let mut info = std::mem::MaybeUninit::<libc::siginfo_t>::zeroed();
        let process_id = libc::id_t::try_from(child.id()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("process id {} cannot be represented as id_t", child.id()),
            )
        })?;
        #[expect(
            unsafe_code,
            reason = "observing Unix child exit without reaping requires libc::waitid"
        )]
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                process_id,
                info.as_mut_ptr(),
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result != 0 {
            return Err(io::Error::last_os_error());
        }
        #[expect(unsafe_code, reason = "successful libc::waitid initializes siginfo_t")]
        let info = unsafe { info.assume_init() };
        #[expect(
            unsafe_code,
            reason = "reading the process id from libc::siginfo_t requires its accessor"
        )]
        let process_id = unsafe { info.si_pid() };
        Ok(process_id != 0)
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
