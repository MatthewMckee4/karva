pub mod hash;
pub mod reader;
pub mod writer;

pub use hash::RunHash;
pub use reader::{AggregatedResults, CacheReader};
pub use writer::CacheWriter;

pub const CACHE_DIR: &str = ".karva_cache";
pub const STATS_FILE: &str = "stats.json";
pub const DIAGNOSTICS_FILE: &str = "diagnostics.txt";
pub const DISCOVER_DIAGNOSTICS_FILE: &str = "discover_diagnostics.txt";

pub fn worker_folder(worker_id: usize) -> String {
    format!("worker-{worker_id}")
}
