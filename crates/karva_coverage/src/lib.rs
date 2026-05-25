//! Line-coverage measurement for karva.
//!
//! Two halves live here:
//!
//! * [`tracer`] runs in the worker process. It installs a Python tracer
//!   (`sys.monitoring` on 3.12+, `sys.settrace` otherwise), records every
//!   executed line under the configured source roots, computes executable
//!   lines via the AST, and writes a per-worker JSON file.
//! * [`report`] runs in the main process. It reads each worker's JSON
//!   file, unions the line sets per source file, and prints a terminal
//!   `Name / Stmts / Miss / Cover` table or writes machine-readable reports.
//!
//! The two halves communicate only through the JSON file format, defined
//! in [`data`].

pub mod data;
pub mod executable;
pub mod report;
pub mod tracer;

pub use report::{combine_and_report, write_cobertura_xml, write_html_report, write_json_report};
pub use tracer::{CoverageConfig, CoverageSession};
