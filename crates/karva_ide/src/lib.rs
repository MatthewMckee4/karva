//! Source-only editor analysis for Karva projects.

mod fixture;

use camino::{Utf8Path, Utf8PathBuf};
use karva_collector::{CollectedModule, CollectionSettings, collect_source};
use ruff_python_ast::PythonVersion;
use ruff_text_size::TextRange;

pub use fixture::{
    FixtureDefinition, FixtureId, FixtureReference, FixtureResolution, FixtureScope,
};

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
    pub module: CollectedModule,

    /// Statically understood fixture declarations.
    pub fixtures: Vec<FixtureDefinition>,

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
    let (fixtures, diagnostics) = fixture::analyze(&module, settings.try_import_fixtures);
    Some(SourceAnalysis {
        module,
        fixtures,
        diagnostics,
    })
}

/// Analyzes a current document against already-collected configuration modules.
///
/// `parents` must be ordered from the project/session root toward the current
/// package, matching runtime fixture lookup precedence.
pub fn analyze_sources(
    current: CollectedModule,
    parents: &[CollectedModule],
    settings: &SourceAnalysisSettings,
) -> SourceAnalysis {
    let parent_modules = parents.iter().collect::<Vec<_>>();
    let (fixtures, diagnostics) =
        fixture::analyze_modules(&current, &parent_modules, settings.try_import_fixtures);
    SourceAnalysis {
        module: current,
        fixtures,
        diagnostics,
    }
}
