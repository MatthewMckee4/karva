//! Shared worker payloads, native artifacts, and context encoding for coverage.
//!
//! Runtime tracing and report rendering depend on this crate independently so
//! changes to either side do not rebuild the other.

#![warn(unreachable_pub)]

pub mod context;
pub mod data;
pub mod native;
