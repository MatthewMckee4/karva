//! Semantic test outcomes and fixture failure context.

use serde::{Deserialize, Serialize};

use crate::Diagnostic;

use super::super::diagnostic::RenderedDiagnostic;
use super::super::kind::IndividualTestResultKind;

/// Semantic outcome of one test, parameterized by diagnostic representation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TestCaseOutcome<D = RenderedDiagnostic> {
    /// Test completed without failure.
    Passed,

    /// Assertion or explicit test failure.
    Failed {
        /// Primary failure diagnostic.
        diagnostic: D,

        /// Additional diagnostics belonging to the same failure.
        #[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
        related: Vec<D>,
    },

    /// Collection, fixture, or execution error rather than a test assertion failure.
    Error {
        /// Primary error diagnostic.
        diagnostic: D,

        /// Additional diagnostics belonging to the same error.
        #[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
        related: Vec<D>,

        /// Fixture failures that explain how this error reached the test.
        #[serde(default = "Vec::new", skip_serializing_if = "Vec::is_empty")]
        fixture_failures: Vec<FixtureFailure>,
    },

    /// Test intentionally did not execute.
    Skipped {
        /// User-provided skip reason, when available.
        reason: Option<String>,
    },
}

impl<D> TestCaseOutcome<D> {
    /// Creates a failure with no related diagnostics.
    pub fn failed(diagnostic: D) -> Self {
        Self::Failed {
            diagnostic,
            related: Vec::new(),
        }
    }

    /// Creates an execution error with no related diagnostics.
    pub fn error(diagnostic: D) -> Self {
        Self::error_with_related(diagnostic, Vec::new())
    }

    /// Creates an execution error retaining secondary diagnostics.
    pub fn error_with_related(diagnostic: D, related: Vec<D>) -> Self {
        Self::error_with_fixture_failures(diagnostic, related, Vec::new())
    }

    /// Creates an execution error with its fixture dependency context.
    pub fn error_with_fixture_failures(
        diagnostic: D,
        related: Vec<D>,
        fixture_failures: Vec<FixtureFailure>,
    ) -> Self {
        Self::Error {
            diagnostic,
            related,
            fixture_failures,
        }
    }

    /// Whether this is an assertion or explicit test failure.
    pub fn is_failed(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    /// Whether this is a collection, fixture, or execution error.
    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error { .. })
    }

    /// Whether this outcome fails or errors rather than passing or skipping.
    pub fn is_non_success(&self) -> bool {
        matches!(self, Self::Failed { .. } | Self::Error { .. })
    }

    /// Whether the test was intentionally skipped.
    pub fn is_skipped(&self) -> bool {
        matches!(self, Self::Skipped { .. })
    }

    /// Returns the primary diagnostic attached to a failure or error.
    pub fn diagnostic(&self) -> Option<&D> {
        match self {
            Self::Failed { diagnostic, .. } | Self::Error { diagnostic, .. } => Some(diagnostic),
            Self::Passed | Self::Skipped { .. } => None,
        }
    }

    /// Returns secondary diagnostics attached to a failure or error.
    pub fn related_diagnostics(&self) -> &[D] {
        match self {
            Self::Failed { related, .. } | Self::Error { related, .. } => related,
            Self::Passed | Self::Skipped { .. } => &[],
        }
    }

    /// Returns fixture failures attached to execution errors.
    pub fn fixture_failures(&self) -> &[FixtureFailure] {
        match self {
            Self::Error {
                fixture_failures, ..
            } => fixture_failures,
            Self::Passed | Self::Failed { .. } | Self::Skipped { .. } => &[],
        }
    }

    /// Maps this semantic outcome into its reporting and statistics category.
    pub fn result_kind(&self) -> IndividualTestResultKind {
        match self {
            Self::Passed => IndividualTestResultKind::Passed,
            Self::Failed { .. } => IndividualTestResultKind::Failed,
            Self::Error { .. } => IndividualTestResultKind::Error,
            Self::Skipped { reason } => IndividualTestResultKind::Skipped {
                reason: reason.clone(),
            },
        }
    }

    pub(super) fn map_diagnostic<T>(self, mut map: impl FnMut(&D) -> T) -> TestCaseOutcome<T> {
        match self {
            Self::Passed => TestCaseOutcome::Passed,
            Self::Failed {
                diagnostic,
                related,
            } => TestCaseOutcome::Failed {
                diagnostic: map(&diagnostic),
                related: related
                    .into_iter()
                    .map(|diagnostic| map(&diagnostic))
                    .collect(),
            },
            Self::Error {
                diagnostic,
                related,
                fixture_failures,
            } => TestCaseOutcome::Error {
                diagnostic: map(&diagnostic),
                related: related
                    .into_iter()
                    .map(|diagnostic| map(&diagnostic))
                    .collect(),
                fixture_failures,
            },
            Self::Skipped { reason } => TestCaseOutcome::Skipped { reason },
        }
    }
}

/// Fixture setup failure and the dependency path that exposed it to a test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureFailure {
    /// Fixture whose setup failed.
    fixture: String,

    /// How the test acquired the fixture.
    usage: FixtureUsage,

    /// Fixture dependency path from the test to the failure.
    dependency_chain: Vec<String>,
}

impl FixtureFailure {
    /// Records a fixture failure and how it reached the test.
    pub fn new(fixture: String, usage: FixtureUsage, dependency_chain: Vec<String>) -> Self {
        Self {
            fixture,
            usage,
            dependency_chain,
        }
    }

    /// Returns the fixture dependency path from the test to the failure.
    pub fn dependency_chain(&self) -> &[String] {
        &self.dependency_chain
    }

    /// Describes the failed fixture relationship for user-facing diagnostics.
    pub fn description(&self) -> String {
        match self.usage {
            FixtureUsage::Required => format!("requires fixture `{}`", self.fixture),
            FixtureUsage::UseFixtures => {
                format!("uses fixture `{}` via `use_fixtures`", self.fixture)
            }
            FixtureUsage::AutoUse => format!("uses auto-use fixture `{}`", self.fixture),
        }
    }
}

/// Mechanism through which a test depends on a fixture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FixtureUsage {
    /// Fixture appears as a test or fixture parameter.
    Required,

    /// Fixture was requested by `@karva.tags.use_fixtures`.
    UseFixtures,

    /// Fixture applies automatically without an explicit request.
    AutoUse,
}

/// Worker-side test outcome retaining structured diagnostics.
pub type TestExecutionOutcome = TestCaseOutcome<Diagnostic>;
