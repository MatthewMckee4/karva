//! Test outcomes, diagnostics, tracebacks, and user-facing result reporting.

mod reporter;
mod result;
#[cfg(feature = "traceback")]
mod traceback;

pub use reporter::{DummyReporter, Reporter, TestCaseReporter};
pub use result::{
    CapturedTestOutput, DisplayFlakyTests, FixtureFailure, FixtureUsage, FlakyTest,
    IndividualTestResultKind, RenderedDiagnostic, TestCaseAttempt, TestCaseOutcome, TestCaseResult,
    TestCaseRetry, TestExecutionAttempt, TestExecutionOutcome, TestExecutionResult, TestResultKind,
    TestResultStats, TestRunResult,
};

#[cfg(feature = "traceback")]
pub use traceback::{Traceback, TracebackFrame, TracebackFrameSource};
