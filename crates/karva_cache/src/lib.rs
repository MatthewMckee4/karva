pub(crate) mod cache;
pub(crate) mod hash;

pub use cache::{
    AggregatedResults, Cache, PruneResult, clean_cache, prune_cache, read_last_failed,
    read_recent_durations, write_last_failed,
};
pub use hash::RunHash;

pub const CACHE_DIR: &str = ".karva_cache";
pub(crate) const STATS_FILE: &str = "stats.json";
pub(crate) const DIAGNOSTICS_FILE: &str = "diagnostics.txt";
pub(crate) const DISCOVER_DIAGNOSTICS_FILE: &str = "discover_diagnostics.txt";
pub(crate) const DURATIONS_FILE: &str = "durations.json";
pub(crate) const FAILED_TESTS_FILE: &str = "failed_tests.json";
const FAIL_FAST_SIGNAL_FILE: &str = "fail-fast";
const LAST_FAILED_FILE: &str = "last-failed.json";

pub(crate) fn worker_folder(worker_id: usize) -> String {
    format!("worker-{worker_id}")
}
