//! Controller-side collection, partitioning, and worker-process orchestration.

mod binary;
mod collection;
mod orchestration;
mod partition;
mod shutdown;
mod worker_args;

pub use orchestration::{ParallelTestConfig, RunOutput, run_parallel_tests};
pub use partition::TestOrdering;
pub use shutdown::shutdown_receiver;

/// Generates a seed spanning the full supported `u64` seed space.
pub fn generate_random_seed() -> u64 {
    fastrand::u64(..)
}
