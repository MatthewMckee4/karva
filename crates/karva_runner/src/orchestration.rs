//! Controller-side collection, worker supervision, and result aggregation.

use std::time::Duration;

mod config;
mod dispatcher;
mod output;
mod planning;
mod process_control;
mod recovery;
mod run;
mod spawn;
mod streams;
mod supervision;
mod termination;
mod worker;

pub use config::{
    FailurePriority, LastFailedSelection, ParallelTestConfig, RunOutput, TestResultRetention,
    WorkerCountSource,
};
pub use run::run_parallel_tests;

// Receipt: worker writes and controller reads each advance every 10 ms. With
// no window the cancellation integration test consistently missed the first
// test checkpoint; five intervals passed 20 consecutive repetitions.
const CANCELLATION_EVENT_SETTLE: Duration = Duration::from_millis(50);
// Receipt: five polls fit inside the 50 ms event-settle window, keeping
// timeout, fail-fast, and cancellation checks responsive without busy-spinning.
const WORKER_POLL_INTERVAL: Duration = Duration::from_millis(10);
