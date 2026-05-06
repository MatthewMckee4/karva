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
//! The drain runs whenever workers exist; the bar is gated on `--show-progress`.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write as _};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Drains per-worker output files and optionally drives a progress bar.
///
/// Drop or call [`Self::finish`] to stop the polling thread, do a final
/// drain so no buffered output is lost, and clear the bar.
pub struct OutputDrain {
    bar: Option<ProgressBar>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl OutputDrain {
    /// Start the drain.
    ///
    /// `mode` selects the bar style; `total_tests` is the initial estimate
    /// of test cases (parametrize-aware counts aren't known up front, so
    /// the bar grows its length dynamically when ticks overrun the estimate).
    /// `num_workers` is the number of worker output files to poll.
    pub fn start(
        mode: ProgressMode,
        total_tests: u64,
        num_workers: usize,
        cache: RunCache,
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

        let handle = {
            let bar = bar.clone();
            let stop = Arc::clone(&stop);
            thread::spawn(move || {
                drain_loop(&output_paths, &cache, total_tests, bar.as_ref(), &stop);
            })
        };

        Self {
            bar,
            stop,
            handle: Some(handle),
        }
    }

    /// Stop polling, drain any remaining whole lines, and clear the bar.
    pub fn finish(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
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
    initial_total: u64,
    bar: Option<&ProgressBar>,
    stop: &AtomicBool,
) {
    let mut streams: Vec<WorkerStream> = output_paths
        .iter()
        .cloned()
        .map(WorkerStream::new)
        .collect();
    let mut current_total = initial_total;

    loop {
        let mut lines: Vec<String> = Vec::new();
        let mut progressed = false;
        for stream in &mut streams {
            if stream.poll(&mut lines) {
                progressed = true;
            }
        }
        emit_lines(&lines, bar);

        if let Some(bar) = bar {
            let completed = cache.completed_count();
            // `initial_total` counts test function definitions; workers tick
            // once per parametrized case, so completion can overrun the
            // estimate. Grow the bar's length to keep position <= len.
            if completed > current_total {
                current_total = completed;
                bar.set_length(current_total);
            }
            bar.set_position(completed);
        }

        if stop.load(Ordering::SeqCst) {
            // Final drain: pick up anything written between the last poll
            // and the worker exiting.
            let mut final_lines: Vec<String> = Vec::new();
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
