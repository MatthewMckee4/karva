//! Test outcomes, diagnostics, tracebacks, and user-facing result reporting.

mod diagnostic;
mod render;
mod reporter;
mod result;
#[cfg(feature = "traceback")]
mod traceback;

pub use diagnostic::{Annotation, Diagnostic, Severity, Span, SubDiagnostic};
pub use render::{DiagnosticFormat, DisplayDiagnosticConfig, render_diagnostic};
pub use reporter::{DummyReporter, Reporter, TestCaseReporter};
pub use result::{
    CapturedTestOutput, DisplayFlakyTests, FixtureFailure, FixtureUsage, FlakyTest,
    IndividualTestResultKind, RenderedDiagnostic, TestCaseAttempt, TestCaseOutcome, TestCaseResult,
    TestCaseRetry, TestExecutionAttempt, TestExecutionOutcome, TestExecutionResult, TestResultKind,
    TestResultStats, TestRunResult, TestRunResultParts,
};

#[cfg(feature = "traceback")]
pub use traceback::{Traceback, TracebackFrame, TracebackFrameSource};
