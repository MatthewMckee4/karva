//! Worker-side Python coverage tracing and executable-line analysis.
//!
//! Persists worker payloads defined by `karva_coverage`; report rendering lives
//! in `karva_coverage_report` and is independent of the embedded interpreter.

#![warn(unreachable_pub)]

mod branches;
pub mod executable;
mod tracer;

pub use tracer::{CoverageConfig, CoveragePhase, CoverageSession};
