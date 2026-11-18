//! Native Python mutation discovery and durable results.
//!
//! This crate never imports Python or modifies application source during discovery.
//! Workers validate compilation and execute each candidate in a fresh interpreter.

mod data;
mod discovery;
mod identity;

pub use data::{Mutant, MutationRecord, MutationResults, Operator, Outcome, fingerprint_file};
pub use discovery::{DiscoveryFilters, discover};
pub use identity::fingerprint;

/// Project-relative directory for saved mutation runs and their command lock.
pub const MUTATION_DIR: &str = ".karva/mutation";

/// Project-relative artifact shared by mutation execution and inspection commands.
pub const MUTATION_RESULTS_PATH: &str = ".karva/mutation/results.json";
