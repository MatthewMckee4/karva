//! Test outcomes, diagnostics, tracebacks, and user-facing result reporting.

mod diagnostic;
mod render;
mod reporter;
mod result;
#[cfg(feature = "traceback")]
mod traceback;

pub use reporter::{DummyReporter, Reporter, TestCaseReporter};
pub use result::{
    AggregatedResults, CapturedTestOutput, DisplayFlakyTests, FixtureFailure, FixtureUsage,
    FlakyTest, IndividualTestResultKind, RenderedDiagnostic, RunResults, TestCaseAttempt,
    TestCaseOutcome, TestCaseResult, TestCaseRetry, TestExecutionAttempt, TestExecutionOutcome,
    TestExecutionResult, TestResultStats,
};

pub use diagnostic::{
    Annotation, Diagnostic, Severity, Span, SubDiagnostic, sort_diagnostics_for_display,
};
pub use render::{DiagnosticFormat, DisplayDiagnosticConfig, render_diagnostic};
#[cfg(feature = "traceback")]
pub use traceback::{Traceback, TracebackFrame, TracebackFrameSource};
