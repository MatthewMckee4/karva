//! Source-only editor analysis for Karva projects.

#![expect(
    dead_code,
    reason = "language-server consumers land in later stack layers"
)]

mod fixture;

use camino::{Utf8Path, Utf8PathBuf};
use karva_collector::{CollectedModule, CollectionSettings, collect_source};
use ruff_python_ast::PythonVersion;
use ruff_text_size::TextRange;

use fixture::FixtureDefinition;

#[cfg(test)]
pub(crate) use fixture::FixtureResolution;

/// Settings required to analyze one Python source document.
#[derive(Clone, Debug)]
pub(crate) struct SourceAnalysisSettings {
    /// Python grammar version used by the project.
    python_version: PythonVersion,

    /// Prefix identifying test functions.
    test_function_prefix: String,

    /// Whether runtime discovery may import fixture providers from test modules.
    try_import_fixtures: bool,
}

/// Stable identifier for a source diagnostic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiagnosticCode {
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
    const fn as_str(self) -> &'static str {
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
pub(crate) struct SourceLocation {
    /// File containing the location.
    path: Utf8PathBuf,

    /// UTF-8 byte range within the file.
    range: TextRange,
}

/// Secondary location explaining a diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RelatedInformation {
    /// Human-readable relationship to the primary diagnostic.
    message: String,

    /// Relevant source location.
    location: SourceLocation,
}

/// A definite source-only Karva diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceDiagnostic {
    /// Stable diagnostic identifier.
    code: DiagnosticCode,

    /// User-facing explanation.
    message: String,

    /// Primary source location.
    location: SourceLocation,

    /// Supporting source locations.
    related: Vec<RelatedInformation>,
}

/// Parsed source plus Karva-specific semantic facts.
#[derive(Debug)]
pub(crate) struct SourceAnalysis {
    /// Collector output retained for later editor features.
    module: CollectedModule,

    /// Statically understood fixture declarations.
    fixtures: Vec<FixtureDefinition>,

    /// Definite diagnostics. Unknown dynamic behavior remains silent.
    diagnostics: Vec<SourceDiagnostic>,
}

/// Analyzes unsaved Python source without importing Python or launching workers.
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
pub(crate) fn analyze_sources(
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
