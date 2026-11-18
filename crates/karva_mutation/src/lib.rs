//! Native Python mutation discovery and durable results.
//!
//! This crate never imports Python or modifies application source during discovery.
//! Workers validate compilation and execute each candidate in a fresh interpreter.

#[cfg(test)]
mod tests;

mod identity;
pub use identity::fingerprint;
