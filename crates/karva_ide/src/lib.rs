//! Editor features built on Karva's shared source analysis.

mod call_hierarchy;
mod code_actions;
mod completion;
mod definition;
mod hover;
mod implementation;
mod occurrences;
mod references;
mod rename;
mod symbols;

pub use call_hierarchy::{
    FixtureHierarchyCall, FixtureHierarchyItem, fixture_hierarchy_items,
    fixture_hierarchy_provider, fixture_incoming_calls, fixture_outgoing_calls,
};
pub use code_actions::{FixtureCodeAction, fixture_code_actions};
pub use completion::{FixtureCompletion, complete_fixtures};
pub use definition::{FixtureDefinitionTarget, fixture_definition};
pub use hover::{FixtureHover, fixture_reference_hovers, hover_fixture};
pub use implementation::{FixtureImplementationTarget, fixture_implementation};
pub use karva_analysis::{
    DiagnosticCode, FixtureId, FixtureOccurrence, FixtureOccurrenceKind, FixtureScope,
    RelatedInformation, SourceAnalysis, SourceAnalysisSettings, SourceDiagnostic, SourceLocation,
    WorkspaceSourceIndex, analyze_source,
};
use karva_analysis::{FixtureResolution, fixture_occurrences};
pub use occurrences::{
    FixtureRenameTarget, fixture_document_highlights, fixture_rename_target, fixture_target,
};
pub use references::{LocatedFixtureOccurrence, fixture_references};
pub use rename::{is_valid_fixture_name, prepare_fixture_rename, rename_fixture};
pub use symbols::{SourceSymbol, SourceSymbolKind, SourceTest, source_symbols, source_tests};

#[cfg(test)]
mod test_support;
#[cfg(test)]
use test_support::{SourceDocument, analyze_source_with_parents, analyze_sources};

#[cfg(test)]
mod import_tests;
