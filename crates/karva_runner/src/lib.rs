mod collection;
mod orchestration;
mod partition;
mod shutdown;

pub use orchestration::{ParallelTestConfig, run_parallel_tests};
