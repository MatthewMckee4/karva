//! Shared source analysis for Karva CLI and editor consumers.
//!
//! This crate resolves fixture providers and Python source relationships without
//! importing test modules, launching workers, or depending on editor features.

use std::sync::Arc;

mod fixture;
mod health;
mod occurrences;
mod source_index;

use camino::{Utf8Path, Utf8PathBuf};
use karva_collector::{CollectedModule, CollectionSettings, collect_source};
use ruff_python_ast::PythonVersion;
use ruff_text_size::TextRange;

pub use fixture::{
    BuiltinFixture, FixtureDefinition, FixtureId, FixtureModel, FixtureReference,
    FixtureResolution, FixtureScope, builtin_fixtures, builtin_info, single_string_content_range,
};
pub use health::{duplicate_parameter_cases, shadowed_definitions, unused_fixtures};
pub use occurrences::{
    FixtureOccurrence, FixtureOccurrenceKind, body_contains_name, body_contains_name_in_body,
    body_has_nested_binding_conflict, body_has_unsupported_bindings,
    expression_contains_name_in_scope, fixture_occurrence, fixture_occurrences, local_bindings,
};
pub use source_index::WorkspaceSourceIndex;

/// Source document used to construct immutable index snapshots in tests.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceDocument {
    path: Utf8PathBuf,
    source_text: String,
}

#[cfg(test)]
impl SourceDocument {
    /// Creates a source document with its stable filesystem path.
    fn new(path: Utf8PathBuf, source_text: String) -> Self {
        Self { path, source_text }
    }

    /// Consumes the document and returns its path and source text.
    fn into_parts(self) -> (Utf8PathBuf, String) {
        (self.path, self.source_text)
    }
}

/// Settings required to analyze one Python source document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceAnalysisSettings {
    /// Python grammar version used by the project.
    pub python_version: PythonVersion,

    /// Prefix identifying test functions.
    pub test_function_prefix: String,

    /// Whether runtime discovery may import fixture providers from test modules.
    pub try_import_fixtures: bool,
}

/// Stable identifier for a source diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagnosticCode {
    /// A fixture has no reachable consumers in a closed source graph.
    UnusedFixture,

    /// Identical literal arguments occur more than once in one parameter dimension.
    DuplicateParameterCase,

    /// An explicit parameter ID is repeated in one parameter dimension.
    DuplicateParameterId,

    /// A later definition replaces a test or fixture in the same Python scope.
    ShadowedDefinition,

    /// Conditional execution may replace a test or fixture.
    PossiblyShadowedDefinition,

    /// Two fixtures in one module resolve to the same public name.
    DuplicateFixture,

    /// A fixture decorator contains a statically invalid argument.
    InvalidFixture,

    /// A fixture or test requires a name with no visible provider.
    MissingFixture,

    /// Fixture dependencies contain a cycle.
    FixtureCycle,

    /// A broader fixture depends on a narrower fixture.
    FixtureScopeMismatch,
}

impl DiagnosticCode {
    /// All supported source diagnostic identifiers, for validating suppressions.
    pub const ALL: &[Self] = &[
        Self::UnusedFixture,
        Self::DuplicateParameterCase,
        Self::DuplicateParameterId,
        Self::ShadowedDefinition,
        Self::PossiblyShadowedDefinition,
        Self::DuplicateFixture,
        Self::InvalidFixture,
        Self::MissingFixture,
        Self::FixtureCycle,
        Self::FixtureScopeMismatch,
    ];

    /// Concrete next step for a source diagnostic.
    pub const fn action(self) -> &'static str {
        match self {
            Self::UnusedFixture => {
                "Remove the fixture, or explicitly export it if it is a public fixture API."
            }
            Self::DuplicateParameterCase => {
                "Remove the repeated literal case, or suppress this rule if repetition is intentional."
            }
            Self::DuplicateParameterId => "Give each explicit case ID a distinct name.",
            Self::ShadowedDefinition | Self::PossiblyShadowedDefinition => {
                "Rename or remove one definition so each test or fixture has a distinct name."
            }
            Self::DuplicateFixture => "Rename or remove the duplicate fixture.",
            Self::InvalidFixture => "Correct the fixture decorator arguments.",
            Self::MissingFixture => "Define or import the required fixture.",
            Self::FixtureCycle => "Remove a dependency from the fixture cycle.",
            Self::FixtureScopeMismatch => "Align the fixture and dependency scopes.",
        }
    }

    /// Returns the stable protocol code.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnusedFixture => "unused-fixture",
            Self::DuplicateParameterCase => "duplicate-parameter-case",
            Self::DuplicateParameterId => "duplicate-parameter-id",
            Self::ShadowedDefinition => "shadowed-definition",
            Self::PossiblyShadowedDefinition => "possibly-shadowed-definition",
            Self::DuplicateFixture => "duplicate-fixture",
            Self::InvalidFixture => "invalid-fixture",
            Self::MissingFixture => "missing-fixture",
            Self::FixtureCycle => "fixture-cycle",
            Self::FixtureScopeMismatch => "fixture-scope-mismatch",
        }
    }
}

/// A source location independent of editor protocol types.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLocation {
    /// File containing the location.
    pub path: Utf8PathBuf,

    /// UTF-8 byte range within the file.
    pub range: TextRange,
}

/// Secondary location explaining a diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelatedInformation {
    /// Human-readable relationship to the primary diagnostic.
    pub message: String,

    /// Relevant source location.
    pub location: SourceLocation,
}

/// A definite source-only Karva diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDiagnostic {
    /// Stable diagnostic identifier.
    pub code: DiagnosticCode,

    /// User-facing explanation.
    pub message: String,

    /// Primary source location.
    pub location: SourceLocation,

    /// Supporting source locations.
    pub related: Vec<RelatedInformation>,
}

/// Parsed source plus Karva-specific semantic facts.
#[derive(Clone, Debug)]
pub struct SourceAnalysis {
    /// Parsed syntax shared across source queries.
    module: Arc<CollectedModule>,

    /// Resolved fixture declarations and provider visibility.
    fixture_model: Arc<FixtureModel>,

    /// Definite diagnostics. Unknown dynamic behavior remains silent.
    pub diagnostics: Vec<SourceDiagnostic>,
}

impl SourceAnalysis {
    /// Syntax collected for this document, shared by diagnostic and editor queries.
    pub fn module(&self) -> &CollectedModule {
        &self.module
    }

    /// Immutable fixture declarations and resolution facts for this document.
    pub fn fixture_model(&self) -> &FixtureModel {
        &self.fixture_model
    }
}

/// Analyzes unsaved Python source without importing Python or launching workers.
pub fn analyze_source(
    path: &Utf8PathBuf,
    project_root: &Utf8Path,
    source_text: String,
    settings: &SourceAnalysisSettings,
) -> Option<SourceAnalysis> {
    let collection_settings = CollectionSettings {
        python_version: settings.python_version,
        test_function_prefix: &settings.test_function_prefix,
        respect_ignore_files: true,
        collect_fixtures: true,
        collect_doctests: false,
    };
    let module = collect_source(path, project_root, source_text, &collection_settings, &[])?;
    let (fixture_model, diagnostics) = fixture::analyze(&module, settings.try_import_fixtures);
    Some(SourceAnalysis {
        module: Arc::new(module),
        fixture_model: Arc::new(fixture_model),
        diagnostics,
    })
}

/// Analyzes a current document against already-collected configuration modules.
///
/// `parents` must be ordered from the project/session root toward the current
/// package, matching runtime fixture lookup precedence.
#[cfg(test)]
pub(crate) fn analyze_sources(
    current: CollectedModule,
    parents: &[CollectedModule],
    settings: &SourceAnalysisSettings,
) -> SourceAnalysis {
    let parent_modules = parents.iter().collect::<Vec<_>>();
    analyze_collected_source(Arc::new(current), &parent_modules, None, settings)
}

#[cfg(test)]
fn analyze_collected_source(
    current: Arc<CollectedModule>,
    parents: &[&CollectedModule],
    builtin_module: Option<&CollectedModule>,
    settings: &SourceAnalysisSettings,
) -> SourceAnalysis {
    let (fixture_model, diagnostics) = fixture::analyze_modules(
        &current,
        parents,
        builtin_module,
        settings.try_import_fixtures,
    );
    SourceAnalysis {
        module: current,
        fixture_model: Arc::new(fixture_model),
        diagnostics,
    }
}
