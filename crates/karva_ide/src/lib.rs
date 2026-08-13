//! Source-only editor analysis for Karva projects.

mod completion;
mod definition;
mod fixture;
mod hover;
mod occurrences;
mod references;
mod rename;
mod source_index;

use camino::{Utf8Path, Utf8PathBuf};
use karva_collector::{CollectedModule, CollectionSettings, collect_source};
use ruff_python_ast::PythonVersion;
use ruff_text_size::TextRange;

pub use completion::{FixtureCompletion, complete_fixtures};
pub use definition::{FixtureDefinitionTarget, fixture_definition};
use fixture::{FixtureDefinition, FixtureResolution};
pub use fixture::{FixtureId, FixtureScope};
pub use hover::{FixtureHover, hover_fixture};
pub use occurrences::{FixtureOccurrence, fixture_target};
pub(crate) use occurrences::{FixtureOccurrenceKind, fixture_occurrences};
pub use references::{LocatedFixtureOccurrence, fixture_references};
pub use rename::prepare_fixture_rename;
pub use source_index::WorkspaceSourceIndex;

/// Owned Python source used as an input to source-only analysis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDocument {
    path: Utf8PathBuf,
    source_text: String,
}

impl SourceDocument {
    /// Creates a source document with its stable filesystem path.
    pub fn new(path: Utf8PathBuf, source_text: String) -> Self {
        Self { path, source_text }
    }

    /// Consumes the document and returns its path and source text.
    fn into_parts(self) -> (Utf8PathBuf, String) {
        (self.path, self.source_text)
    }
}

/// Settings required to analyze one Python source document.
#[derive(Clone, Debug)]
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
    /// Returns the stable protocol code.
    pub const fn as_str(self) -> &'static str {
        match self {
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
#[derive(Debug)]
pub struct SourceAnalysis {
    /// Collector output retained for later editor features.
    module: CollectedModule,

    /// Statically understood fixture declarations.
    fixtures: Vec<FixtureDefinition>,

    /// Fixture declarations visible to the current module, including parents.
    visible_fixtures: Vec<FixtureDefinition>,

    fixture_completion_blocked_names: std::collections::HashSet<String>,

    fixture_completion_builtins_visible: bool,

    /// Definite diagnostics. Unknown dynamic behavior remains silent.
    pub diagnostics: Vec<SourceDiagnostic>,
}

/// Analyzes unsaved Python source without importing Python or launching workers.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "completion and navigation consumers land in later stack layers"
    )
)]
pub(crate) fn analyze_source(
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
    let (fixtures, diagnostics) = fixture::analyze(&module, settings.try_import_fixtures);
    let visible_fixtures = fixture::visible_fixtures(&module, &[], settings.try_import_fixtures);
    Some(SourceAnalysis {
        module,
        fixtures,
        visible_fixtures: visible_fixtures.definitions,
        fixture_completion_blocked_names: visible_fixtures.blocked_names,
        fixture_completion_builtins_visible: visible_fixtures.builtins_visible,
        diagnostics,
    })
}

/// Analyzes an unsaved source document with fixture providers from ancestor
/// `conftest.py` documents.
///
/// `parents` must be ordered from the project/session root toward the current
/// package. The returned module is the current document; parent modules are
/// used to resolve fixture references and retain their source locations.
pub fn analyze_source_with_parents(
    current: SourceDocument,
    parents: impl IntoIterator<Item = SourceDocument>,
    project_root: &Utf8Path,
    settings: &SourceAnalysisSettings,
) -> Option<SourceAnalysis> {
    let collection_settings = CollectionSettings {
        python_version: settings.python_version,
        test_function_prefix: &settings.test_function_prefix,
        respect_ignore_files: true,
        collect_fixtures: true,
        collect_doctests: false,
    };
    let (current_path, current_source) = current.into_parts();
    let current = collect_source(
        &current_path,
        project_root,
        current_source,
        &collection_settings,
        &[],
    )?;

    let parent_modules = parents
        .into_iter()
        .filter_map(|parent| {
            let (path, source_text) = parent.into_parts();
            collect_source(&path, project_root, source_text, &collection_settings, &[])
        })
        .collect::<Vec<_>>();
    let parent_modules = parent_modules.iter().collect::<Vec<_>>();
    let (fixtures, diagnostics) =
        fixture::analyze_modules(&current, &parent_modules, settings.try_import_fixtures);
    let visible_fixtures =
        fixture::visible_fixtures(&current, &parent_modules, settings.try_import_fixtures);
    Some(SourceAnalysis {
        module: current,
        fixtures,
        visible_fixtures: visible_fixtures.definitions,
        fixture_completion_blocked_names: visible_fixtures.blocked_names,
        fixture_completion_builtins_visible: visible_fixtures.builtins_visible,
        diagnostics,
    })
}

/// Analyzes a current document against already-collected configuration modules.
///
/// `parents` must be ordered from the project/session root toward the current
/// package, matching runtime fixture lookup precedence.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "workspace source indexing lands in a later stack layer"
    )
)]
pub(crate) fn analyze_sources(
    current: CollectedModule,
    parents: &[CollectedModule],
    settings: &SourceAnalysisSettings,
) -> SourceAnalysis {
    let parent_modules = parents.iter().collect::<Vec<_>>();
    analyze_collected_source(current, &parent_modules, settings)
}

pub(crate) fn analyze_collected_source(
    current: CollectedModule,
    parents: &[&CollectedModule],
    settings: &SourceAnalysisSettings,
) -> SourceAnalysis {
    let (fixtures, diagnostics) =
        fixture::analyze_modules(&current, parents, settings.try_import_fixtures);
    let visible_fixtures =
        fixture::visible_fixtures(&current, parents, settings.try_import_fixtures);
    SourceAnalysis {
        module: current,
        fixtures,
        visible_fixtures: visible_fixtures.definitions,
        fixture_completion_blocked_names: visible_fixtures.blocked_names,
        fixture_completion_builtins_visible: visible_fixtures.builtins_visible,
        diagnostics,
    }
}
