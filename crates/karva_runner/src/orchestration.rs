//! Controller-side collection, worker supervision, and result aggregation.

use std::time::Duration;

mod config;
mod dispatcher;
mod output;
mod planning;
mod process_control;
mod run;
mod spawn;
mod streams;
mod supervision;
mod termination;
mod worker;

pub use config::{ParallelTestConfig, RunOutput, TestResultRetention};
pub use run::run_parallel_tests;

// Receipt: worker writes and controller reads each advance every 10 ms. With
// no window the cancellation integration test consistently missed the first
// test checkpoint; five intervals passed 20 consecutive repetitions.
const CANCELLATION_EVENT_SETTLE: Duration = Duration::from_millis(50);
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(10);
