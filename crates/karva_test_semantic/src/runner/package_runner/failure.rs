//! Test-owned execution errors shared across runner lifecycle boundaries.

use karva_diagnostic::{FixtureFailure, TestExecutionOutcome};
use ruff_db::diagnostic::Diagnostic;

/// Primary test error plus related diagnostics and fixture causality.
///
/// Keeping these fields together prevents package, module, and attempt paths
/// from independently splitting and rebuilding the same error state.
#[derive(Clone)]
pub(super) struct TestError {
    /// Primary diagnostic displayed for every affected test.
    diagnostic: Diagnostic,

    /// Diagnostics caused by the same failure after the primary error.
    related: Vec<Diagnostic>,

    /// Fixture relationships through which this error reached the test.
    fixture_failures: Vec<FixtureFailure>,
}

impl TestError {
    /// Creates an error with no related diagnostics or fixture causality.
    pub(super) fn new(diagnostic: Diagnostic) -> Self {
        Self {
            diagnostic,
            related: Vec::new(),
            fixture_failures: Vec::new(),
        }
    }

    /// Creates an error caused by one or more fixture setup failures.
    pub(super) fn from_fixture_failures(
        diagnostic: Diagnostic,
        related: Vec<Diagnostic>,
        fixture_failures: Vec<FixtureFailure>,
    ) -> Self {
        Self {
            diagnostic,
            related,
            fixture_failures,
        }
    }

    /// Appends diagnostics produced after the primary error, such as teardown failures.
    #[must_use]
    pub(super) fn with_related(
        mut self,
        diagnostics: impl IntoIterator<Item = Diagnostic>,
    ) -> Self {
        self.related.extend(diagnostics);
        self
    }

    /// Converts this error into an owned test outcome.
    pub(super) fn into_outcome(self) -> TestExecutionOutcome {
        TestExecutionOutcome::error_with_fixture_failures(
            self.diagnostic,
            self.related,
            self.fixture_failures,
        )
    }
}
