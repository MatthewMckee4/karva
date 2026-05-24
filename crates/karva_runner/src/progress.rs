//! Live progress display + serialized worker output.
//!
//! Workers write per-test result lines to per-worker `output` files (via
//! `FileLineSink`) and append a single byte to per-worker `progress` files
//! when each test finishes (via `ProgressTrackingReporter`). The orchestrator
//! drains those files from a single background thread:
//!
//! - reads new bytes from each worker's `output` file, splits on `\n`, and
//!   prints whole lines to its own stdout — preventing worker output from
//!   interleaving on a shared terminal (multiple processes locking their
//!   own stdouts does not actually serialize writes; see issue #502)
//! - reads the aggregate `progress` file length to drive the optional
//!   `indicatif` progress bar on stderr
//!
//! Worker stdout pipes are also drained: each worker is spawned with
//! `Stdio::piped()` for stdout so anything the worker writes outside the
//! reporter (e.g. Python `print()` from user tests) flows through a dedicated
//! reader thread into the orchestrator's stdout under the same
//! `bar.suspend(...)` serialization. Stderr stays inherited so worker tracing
//! lines retain their real-time ordering relative to the orchestrator's own
//! tracing output.
//!
//! The drain runs whenever workers exist; the bar is gated on `--show-progress`.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write as _};
use std::process::ChildStdout;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use camino::Utf8PathBuf;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use karva_cache::RunCache;
use karva_logging::ProgressMode;

const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Refresh frequency for the bar/counter.
///
/// Higher than [`POLL_INTERVAL`] so the display redraw rate is bounded
/// independently of how fast we observe count changes.
const STEADY_TICK: Duration = Duration::from_millis(120);

/// Captured stdout pipe for a single worker child process.
///
/// Held by the orchestrator until [`OutputDrain::start`] hands ownership to
/// a per-pipe reader thread.
pub struct WorkerPipes {
    pub stdout: Option<ChildStdout>,
}

/// Drains per-worker output files and pipes, and optionally drives a
/// progress bar.
///
/// Drop or call [`Self::finish`] to stop the polling thread, do a final
/// drain so no buffered output is lost, and clear the bar.
pub struct OutputDrain {
    bar: Option<ProgressBar>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    pipe_handles: Vec<JoinHandle<()>>,
}

impl OutputDrain {
    /// Start the drain.
    ///
    /// `mode` selects the bar style; `total_tests` is the count of test
    /// function definitions (matching the per-function tick semantics in
    /// `notify_test_completed`). `num_workers` is the number of worker
    /// output files to poll. `pipes` are the captured stdout/stderr pipes
    /// for each worker — they MUST come from children spawned with
    /// `Stdio::piped()` so the orchestrator owns their read ends.
    pub fn start(
        mode: ProgressMode,
        total_tests: u64,
        num_workers: usize,
        cache: RunCache,
        pipes: Vec<WorkerPipes>,
    ) -> Self {
        let bar = if total_tests > 0 && !matches!(mode, ProgressMode::None) {
            let b = ProgressBar::with_draw_target(Some(total_tests), ProgressDrawTarget::stderr());
            b.set_style(style_for(mode));
            if matches!(mode, ProgressMode::Bar) {
                b.set_message("Testing");
            }
            b.enable_steady_tick(STEADY_TICK);
            Some(b)
        } else {
            None
        };

        let stop = Arc::new(AtomicBool::new(false));

        let output_paths: Vec<Utf8PathBuf> =
            (0..num_workers).map(|id| cache.output_file(id)).collect();

        let (stdout_tx, stdout_rx) = mpsc::channel::<String>();

        let mut pipe_handles: Vec<JoinHandle<()>> = Vec::new();
        for pipe in pipes {
            if let Some(out) = pipe.stdout {
                let tx = stdout_tx.clone();
                pipe_handles.push(thread::spawn(move || forward_pipe(out, &tx)));
            }
        }
        // Drop the original sender so the receiver disconnects once every
        // pipe-reader thread has exited (each holds its own clone of `tx`).
        drop(stdout_tx);

        let handle = {
            let bar = bar.clone();
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                drain_loop(&output_paths, &cache, bar.as_ref(), &stop, &stdout_rx);
            })
        };

        Self {
            bar,
            stop,
            handle: Some(handle),
            pipe_handles,
        }
    }

    /// Stop polling, drain any remaining whole lines, and clear the bar.
    pub fn finish(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        // Pipe readers exit on EOF when their worker closes its end. Callers
        // are expected to have already waited on / killed every worker before
        // reaching shutdown, so joining the readers first guarantees the last
        // pipe lines are queued before we tell the drain loop to stop.
        for handle in self.pipe_handles.drain(..) {
            let _ = handle.join();
        }
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        if let Some(bar) = self.bar.take() {
            bar.finish_and_clear();
        }
    }
}

impl Drop for OutputDrain {
    fn drop(&mut self) {
        if !self.stop.load(Ordering::SeqCst) {
            self.shutdown();
        }
    }
}

/// Read a worker pipe to EOF, forwarding each line into `tx`.
///
/// `read_until('\n')` + `from_utf8_lossy` keeps non-UTF-8 bytes from
/// dropping the line entirely (matching the file path's behaviour).
fn forward_pipe<R: Read + Send>(reader: R, tx: &mpsc::Sender<String>) {
    let mut reader = BufReader::new(reader);
    let mut buf: Vec<u8> = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => return,
            Ok(_) => {
                if buf.last() == Some(&b'\n') {
                    buf.pop();
                }
                let line = String::from_utf8_lossy(&buf).into_owned();
                if tx.send(line).is_err() {
                    return;
                }
            }
            Err(_) => return,
        }
    }
}

/// Per-worker drain state: open file handle (lazily) + a buffer for the
/// trailing partial line that hasn't yet been newline-terminated.
struct WorkerStream {
    path: Utf8PathBuf,
    file: Option<File>,
    offset: u64,
    partial: Vec<u8>,
}

impl WorkerStream {
    fn new(path: Utf8PathBuf) -> Self {
        Self {
            path,
            file: None,
            offset: 0,
            partial: Vec::new(),
        }
    }

    /// Read whatever has been appended since the last poll, push complete
    /// lines into `out`, and retain any trailing partial line for next time.
    /// Returns `true` when at least one byte was read.
    fn poll(&mut self, out: &mut Vec<String>) -> bool {
        if self.file.is_none() {
            if !self.path.exists() {
                return false;
            }
            match File::open(&self.path) {
                Ok(f) => self.file = Some(f),
                Err(_) => return false,
            }
        }

        let Some(file) = self.file.as_mut() else {
            return false;
        };

        if file.seek(SeekFrom::Start(self.offset)).is_err() {
            return false;
        }

        let mut buf = Vec::new();
        let Ok(n) = file.read_to_end(&mut buf) else {
            return false;
        };
        if n == 0 {
            return false;
        }
        self.offset += n as u64;

        let mut start = 0usize;
        for (i, byte) in buf.iter().enumerate() {
            if *byte == b'\n' {
                let line_bytes = if self.partial.is_empty() {
                    &buf[start..i]
                } else {
                    self.partial.extend_from_slice(&buf[start..i]);
                    self.partial.as_slice()
                };
                let line = String::from_utf8_lossy(line_bytes).into_owned();
                out.push(line);
                if !self.partial.is_empty() {
                    self.partial.clear();
                }
                start = i + 1;
            }
        }
        if start < buf.len() {
            self.partial.extend_from_slice(&buf[start..]);
        }

        true
    }
}

fn drain_loop(
    output_paths: &[Utf8PathBuf],
    cache: &RunCache,
    bar: Option<&ProgressBar>,
    stop: &AtomicBool,
    stdout_rx: &mpsc::Receiver<String>,
) {
    let mut streams: Vec<WorkerStream> = output_paths
        .iter()
        .cloned()
        .map(WorkerStream::new)
        .collect();

    loop {
        let mut lines: Vec<String> = Vec::new();
        let mut progressed = false;
        // Drain the pipe channel before polling result files: a `print()`
        // inside a test runs *before* the test returns and the reporter
        // writes its result line, so pipe lines should appear before the
        // file lines from the same iteration.
        while let Ok(line) = stdout_rx.try_recv() {
            lines.push(line);
            progressed = true;
        }
        for stream in &mut streams {
            if stream.poll(&mut lines) {
                progressed = true;
            }
        }
        emit_lines(&lines, bar);

        if let Some(bar) = bar {
            bar.set_position(cache.completed_count());
        }

        if stop.load(Ordering::SeqCst) {
            // Final drain: pick up anything written between the last poll
            // and the worker exiting.
            let mut final_lines: Vec<String> = Vec::new();
            while let Ok(line) = stdout_rx.try_recv() {
                final_lines.push(line);
            }
            for stream in &mut streams {
                stream.poll(&mut final_lines);
            }
            emit_lines(&final_lines, bar);
            if let Some(bar) = bar {
                bar.set_position(cache.completed_count());
            }
            break;
        }

        if !progressed {
            thread::sleep(POLL_INTERVAL);
        }
    }
}

fn emit_lines(lines: &[String], bar: Option<&ProgressBar>) {
    if lines.is_empty() {
        return;
    }
    let write = || {
        let mut out = std::io::stdout().lock();
        for line in lines {
            let _ = writeln!(out, "{line}");
        }
    };
    // Reporter lines belong on stdout, but the bar redraws on stderr — without
    // suspending the bar, its tick can clobber the cursor mid-write. `suspend`
    // clears the bar from stderr, runs the closure, then re-renders.
    if let Some(bar) = bar {
        bar.suspend(write);
    } else {
        write();
    }
}

fn style_for(mode: ProgressMode) -> ProgressStyle {
    match mode {
        ProgressMode::Bar => {
            ProgressStyle::with_template("{msg:8.dim} {bar:60.green/dim} {pos}/{len} tests")
                .expect("hardcoded template is valid")
                .progress_chars("--")
        }
        ProgressMode::Counter => {
            ProgressStyle::with_template("{pos}/{len} tests").expect("hardcoded template is valid")
        }
        // unreachable: `start` constructs no bar for `None`. Fall back to
        // counter so we never panic if a future caller bypasses that gate.
        ProgressMode::None => {
            ProgressStyle::with_template("{pos}/{len} tests").expect("hardcoded template is valid")
        }
    }
}
