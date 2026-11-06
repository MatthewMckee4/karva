//! Native Python mutation discovery and durable results.
//!
//! This crate never imports Python or modifies application source during discovery.
//! Workers validate compilation and execute each candidate in a fresh interpreter.

mod data;
mod discovery;

pub use data::{
    Mutant, MutationRecord, MutationResults, Operator, Outcome, fingerprint, fingerprint_file,
};
pub use discovery::{DiscoveryFilters, discover};
