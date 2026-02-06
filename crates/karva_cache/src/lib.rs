pub(crate) mod cache;
pub(crate) mod hash;

pub use cache::{AggregatedResults, Cache, read_recent_durations};
pub use hash::RunHash;

pub const CACHE_DIR: &str = ".karva_cache";
pub(crate) const STATS_FILE: &str = "stats.json";
pub(crate) const DIAGNOSTICS_FILE: &str = "diagnostics.txt";
pub(crate) const DISCOVER_DIAGNOSTICS_FILE: &str = "discover_diagnostics.txt";
pub(crate) const DURATIONS_FILE: &str = "durations.json";

pub(crate) fn worker_folder(worker_id: usize) -> String {
    format!("worker-{worker_id}")
}
