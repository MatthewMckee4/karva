//! Live progress display for parallel test runs.
//!
//! Workers append one byte to a per-worker progress file each time a test
//! completes (see `ProgressTrackingReporter`). A polling thread in the
//! orchestrator reads the aggregate completion count and renders it on
//! stderr — either as a one-line counter or an indicatif progress bar.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use karva_cache::RunCache;
use karva_logging::ProgressMode;

const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Refresh frequency for the bar/counter.
///
/// Higher than [`POLL_INTERVAL`] so the display redraw rate is bounded
/// independently of how fast we observe count changes.
const STEADY_TICK: Duration = Duration::from_millis(120);

/// Owns the indicatif handle and the polling thread that drives it.
///
/// Drop or call [`Self::finish`] to stop the thread and clear the bar
/// before printing the run summary.
pub struct ProgressDisplay {
    bar: ProgressBar,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl ProgressDisplay {
    /// Start a progress display.
    ///
    /// Returns `None` when `mode` is [`ProgressMode::None`] or when there
    /// are no tests to run — neither case warrants a live display.
    pub fn start(mode: ProgressMode, total_tests: u64, cache: RunCache) -> Option<Self> {
        if total_tests == 0 || matches!(mode, ProgressMode::None) {
            return None;
        }

        let bar = ProgressBar::with_draw_target(Some(total_tests), ProgressDrawTarget::stderr());
        bar.set_style(style_for(mode));
        bar.enable_steady_tick(STEADY_TICK);

        let stop = Arc::new(AtomicBool::new(false));

        let handle = {
            let bar = bar.clone();
            let stop = Arc::clone(&stop);
            thread::spawn(move || poll_loop(&bar, &stop, &cache, total_tests))
        };

        Some(Self {
            bar,
            stop,
            handle: Some(handle),
        })
    }

    /// Stop polling and clear the bar from the terminal.
    pub fn finish(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
        self.bar.finish_and_clear();
    }
}

impl Drop for ProgressDisplay {
    fn drop(&mut self) {
        if !self.stop.load(Ordering::SeqCst) {
            self.shutdown();
        }
    }
}

fn poll_loop(bar: &ProgressBar, stop: &AtomicBool, cache: &RunCache, total: u64) {
    loop {
        let completed = cache.completed_count().min(total);
        bar.set_position(completed);

        if stop.load(Ordering::SeqCst) || completed >= total {
            break;
        }

        thread::sleep(POLL_INTERVAL);
    }
}

fn style_for(mode: ProgressMode) -> ProgressStyle {
    match mode {
        ProgressMode::Bar => ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos}/{len} tests",
        )
        .expect("hardcoded template is valid")
        .progress_chars("=> "),
        ProgressMode::Counter => {
            ProgressStyle::with_template("{pos}/{len} tests").expect("hardcoded template is valid")
        }
        // unreachable: `start` returns None for `None`. Fall back to counter
        // so we never panic if a future caller bypasses that gate.
        ProgressMode::None => {
            ProgressStyle::with_template("{pos}/{len} tests").expect("hardcoded template is valid")
        }
    }
}
