//! Test outcomes, source-backed diagnostics, and user-facing result reporting.

mod diagnostic;
mod render;
mod reporter;
mod result;

pub use reporter::{DummyReporter, Reporter, TestCaseReporter};
pub use result::{
    AggregatedResults, CapturedTestOutput, DisplayFlakyTests, FixtureFailure, FixtureUsage,
    FlakyTest, IndividualTestResultKind, RenderedDiagnostic, RunResults, TestCaseAttempt,
    TestCaseOutcome, TestCaseResult, TestCaseRetry, TestExecutionAttempt, TestExecutionOutcome,
    TestExecutionResult, TestResultStats, TestWarning,
};

pub use diagnostic::{
    Annotation, Diagnostic, Severity, Span, SubDiagnostic, sort_diagnostics_for_display,
};
pub use render::{DiagnosticFormat, DisplayDiagnosticConfig, render_diagnostic};
