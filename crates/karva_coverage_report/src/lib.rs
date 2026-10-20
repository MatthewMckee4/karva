//! Controller-side coverage analysis and terminal or machine-readable reports.
//!
//! Consumes the artifacts defined by `karva_coverage` without linking Python or
//! depending on worker tracing and source instrumentation.

#![warn(unreachable_pub)]

mod report;

pub use report::{
    CoverageAnalysis, CoverageFilters, CoverageReportFormat, CoverageReportOptions,
    CoverageReportSort, HtmlReportOptions, JsonReportOptions,
};
