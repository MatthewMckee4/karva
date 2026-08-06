//! Shared on-disk cache for controller and worker processes.

pub(crate) mod artifact;
pub(crate) mod cache;
pub(crate) mod hash;

pub use cache::{
    PruneResult, RunArtifacts, clean_cache, prune_cache, read_last_failed, read_random_seed,
    read_recent_durations, write_durations, write_last_failed, write_random_seed,
};
pub use hash::RunHash;

/// The directory name used for the cache, relative to the project root.
pub const CACHE_DIR: &str = ".karva_cache";

/// Filename prefix for per-run sub-directories of the cache.
pub(crate) const RUN_PREFIX: &str = "run-";

/// Filename prefix for per-worker sub-directories of a run directory.
pub(crate) const WORKER_PREFIX: &str = "worker-";

/// Returns the conventional sub-directory name for a worker within a run directory.
pub(crate) fn worker_folder(worker_id: usize) -> String {
    format!("{WORKER_PREFIX}{worker_id}")
}
