//! Source-only editor analysis for Karva projects.

use std::sync::{Arc, OnceLock};

mod call_hierarchy;
mod code_actions;
mod completion;
mod definition;
mod fixture;
mod hover;
mod implementation;
mod occurrences;
mod references;
mod rename;
mod semantic;
mod source_index;
mod symbols;

use camino::{Utf8Path, Utf8PathBuf};
use karva_collector::{CollectedModule, CollectionSettings, collect_source};
use ruff_python_ast::PythonVersion;
use ruff_text_size::TextRange;

pub use call_hierarchy::{
    FixtureHierarchyCall, FixtureHierarchyItem, fixture_hierarchy_items,
    fixture_hierarchy_provider, fixture_incoming_calls, fixture_outgoing_calls,
};
pub use code_actions::{FixtureCodeAction, fixture_code_actions};
pub use completion::{FixtureCompletion, complete_fixtures};
pub use definition::{FixtureDefinitionTarget, fixture_definition};
pub use fixture::{FixtureId, FixtureScope};
use fixture::{FixtureModel, FixtureResolution};
pub use hover::{FixtureHover, fixture_reference_hovers, hover_fixture};
pub use implementation::{FixtureImplementationTarget, fixture_implementation};
pub(crate) use occurrences::fixture_occurrences;
pub use occurrences::{
    FixtureOccurrence, FixtureOccurrenceKind, FixtureRenameTarget, fixture_document_highlights,
    fixture_rename_target, fixture_target,
};
pub use references::{LocatedFixtureOccurrence, fixture_references};
pub use rename::{is_valid_fixture_name, prepare_fixture_rename, rename_fixture};
pub use semantic::PythonSemanticCache;
pub use source_index::WorkspaceSourceIndex;
pub use symbols::{SourceSymbol, SourceSymbolKind, SourceTest, source_symbols, source_tests};

/// Owned Python source used as an input to source-only analysis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDocument {
    path: Utf8PathBuf,
    source_text: String,
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by source-analysis unit-test helpers")
)]
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
#[derive(Clone, Debug)]
pub struct SourceAnalysis {
    /// Collector output retained for later editor features.
    module: Arc<CollectedModule>,

    /// Resolved fixture declarations and provider visibility.
    fixture_model: Arc<FixtureModel>,

    /// Source view used for lazy ty queries after fixture resolution.
    semantics: Arc<semantic::PythonSemantics>,

    /// Shared occurrences avoid recomputing framework-to-Python bindings for cached requests.
    occurrences: Arc<OnceLock<Vec<FixtureOccurrence>>>,

    /// Definite diagnostics. Unknown dynamic behavior remains silent.
    pub diagnostics: Vec<SourceDiagnostic>,
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
    WorkspaceSourceIndex::from_shared_modules(
        project_root.to_owned(),
        settings.clone(),
        [Arc::new(module)],
    )
    .analyze(path)
}

/// Analyzes an unsaved source document with fixture providers from ancestor
/// `conftest.py` documents.
///
/// `parents` must be ordered from the project/session root toward the current
/// package. The returned module is the current document; parent modules are
/// used to resolve fixture references and retain their source locations.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "used by source-analysis unit-test helpers")
)]
pub(crate) fn analyze_source_with_parents(
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

    let parent_modules = parents.into_iter().filter_map(|parent| {
        let (path, source_text) = parent.into_parts();
        collect_source(&path, project_root, source_text, &collection_settings, &[])
    });
    WorkspaceSourceIndex::from_shared_modules(
        project_root.to_owned(),
        settings.clone(),
        parent_modules.chain([current]).map(Arc::new),
    )
    .analyze(&current_path)
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
    let path = current.path.path().clone();
    let root = parents
        .first()
        .copied()
        .unwrap_or(&current)
        .path
        .path()
        .parent()
        .unwrap_or_else(|| Utf8Path::new("/"))
        .to_owned();
    let modules = parents
        .iter()
        .map(|module| Arc::new((*module).clone()))
        .chain(builtin_module.map(|module| Arc::new(module.clone())))
        .chain([current]);
    WorkspaceSourceIndex::from_shared_modules(root, settings.clone(), modules)
        .analyze(&path)
        .expect("current source is indexed")
}

#[cfg(test)]
mod import_tests;
