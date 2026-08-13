//! Mutable language-server state.

#![expect(
    clippy::redundant_pub_crate,
    reason = "server endpoints consume session APIs across private sibling modules"
)]

pub(super) mod client;
mod index;
mod request_queue;
mod source_index;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use karva_ide::{
    SourceAnalysis, SourceAnalysisSettings, SourceDocument, WorkspaceSourceIndex,
    analyze_source_with_parents,
};
use lsp_types::{LanguageKind, MarkupKind, TextDocumentContentChangeEvent, Uri, WorkspaceFolder};

use crate::workspace::{WorkspaceError, Workspaces, uri_to_path};
use crate::{PositionEncoding, TextDocument};

use self::index::Index;
use self::request_queue::RequestQueue;
use self::source_index::{SourceIndexCache, SourceIndexScope};

pub use self::request_queue::RequestCancellationToken;
use self::source_index::{PreparedSourceIndex, SourceIndexError};

/// Identity of the project source state captured for a background request.
#[derive(Clone, Debug)]
pub(crate) struct SourceIndexRevision(Arc<()>);

impl Default for SourceIndexRevision {
    fn default() -> Self {
        Self(Arc::new(()))
    }
}

/// Document and project state that must remain current for a response.
#[derive(Clone, Debug)]
pub(crate) struct DocumentSnapshotVersion {
    uri: Uri,
    document_version: i32,
    source_index_revision: SourceIndexRevision,
}

/// Failure to apply a document or workspace notification.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    /// The client referenced a document without opening it first.
    #[error("document is not open: {0}")]
    DocumentNotOpen(Uri),

    /// The client referenced a workspace without opening it first.
    #[error("workspace folder is not open: {0}")]
    WorkspaceNotOpen(Uri),

    /// The document change violated its version contract.
    #[error(transparent)]
    DocumentChange(#[from] crate::document::DocumentChangeError),

    /// Project discovery or configuration resolution failed.
    #[error(transparent)]
    Workspace(#[from] WorkspaceError),

    /// Workspace source discovery or collection failed.
    #[error(transparent)]
    SourceIndex(#[from] SourceIndexError),
}

/// Source analysis and the text needed to map every result back to an editor.
#[derive(Debug)]
pub(super) struct SourceAnalysisSnapshot {
    /// Analysis for the current open document.
    pub(super) analysis: SourceAnalysis,

    /// Immutable project source snapshot used by the analysis.
    pub(super) source_index: Arc<WorkspaceSourceIndex>,
}

/// Lightweight analysis used by synchronous diagnostics until diagnostic
/// scheduling moves off the event loop.
pub(super) struct DiagnosticAnalysisSnapshot {
    /// Analysis for the current open document.
    pub(super) analysis: SourceAnalysis,

    /// Source text keyed by each analyzed path.
    pub(super) sources: HashMap<Utf8PathBuf, String>,
}

#[derive(Clone, Copy, Debug)]
enum HierarchicalDocumentSymbols {
    Supported,
    Unsupported,
}

/// Owned inputs for source analysis that can move off the event-loop thread.
#[derive(Debug)]
pub(super) struct PreparedSourceAnalysis {
    current_path: Utf8PathBuf,
    current_source: String,
    source_index: PreparedSourceIndex,
    document_uri: Uri,
    document_version: i32,
    source_index_revision: SourceIndexRevision,
}

impl PreparedSourceAnalysis {
    pub(super) fn source_text(&self) -> &str {
        &self.current_source
    }

    pub(super) fn response_version(&self) -> DocumentSnapshotVersion {
        DocumentSnapshotVersion {
            uri: self.document_uri.clone(),
            document_version: self.document_version,
            source_index_revision: self.source_index_revision.clone(),
        }
    }

    /// Builds the immutable project source snapshot on a worker thread.
    fn into_source_index(
        self,
        cancellation: &RequestCancellationToken,
    ) -> Result<Arc<WorkspaceSourceIndex>, SessionError> {
        Ok(self.source_index.build(cancellation)?)
    }

    /// Reads provider files and computes source semantics away from the event loop.
    pub(super) fn analyze(
        self,
        cancellation: &RequestCancellationToken,
    ) -> Result<Option<SourceAnalysisSnapshot>, SessionError> {
        let current_path = self.current_path.clone();
        let source_index = self.into_source_index(cancellation)?;
        let Some(analysis) = source_index.analyze(&current_path) else {
            return Ok(None);
        };
        Ok(Some(SourceAnalysisSnapshot {
            analysis,
            source_index,
        }))
    }
}

/// Mutable state owned by the language-server event loop.
#[derive(Debug)]
pub struct Session {
    index: Index,
    position_encoding: PositionEncoding,
    hover_markup_kind: MarkupKind,
    shutdown_requested: bool,
    request_queue: RequestQueue,
    supports_diagnostic_related_information: bool,
    hierarchical_document_symbols: HierarchicalDocumentSymbols,
    workspaces: Workspaces,
    published_diagnostic_paths: HashSet<Utf8PathBuf>,
    cache_source_indexes: bool,
    source_indexes: HashMap<(Utf8PathBuf, SourceIndexScope), SourceIndexCache>,
    source_index_revision: SourceIndexRevision,
}

impl Session {
    pub(super) fn new(
        position_encoding: PositionEncoding,
        hover_markup_kind: MarkupKind,
        supports_diagnostic_related_information: bool,
        supports_hierarchical_document_symbols: bool,
        workspaces: Workspaces,
    ) -> Self {
        Self {
            index: Index::new(workspaces.folders().cloned()),
            position_encoding,
            hover_markup_kind,
            request_queue: RequestQueue::default(),
            shutdown_requested: false,
            supports_diagnostic_related_information,
            hierarchical_document_symbols: if supports_hierarchical_document_symbols {
                HierarchicalDocumentSymbols::Supported
            } else {
                HierarchicalDocumentSymbols::Unsupported
            },
            workspaces,
            published_diagnostic_paths: HashSet::new(),
            cache_source_indexes: false,
            source_indexes: HashMap::new(),
            source_index_revision: SourceIndexRevision::default(),
        }
    }

    pub(super) fn is_shutdown_requested(&self) -> bool {
        self.shutdown_requested
    }

    pub(super) fn request_shutdown(&mut self) {
        self.shutdown_requested = true;
    }

    pub(super) fn request_queue_mut(&mut self) -> &mut RequestQueue {
        &mut self.request_queue
    }

    pub(super) fn request_queue(&self) -> &RequestQueue {
        &self.request_queue
    }

    pub(super) fn position_encoding(&self) -> PositionEncoding {
        self.position_encoding
    }

    pub(super) fn hover_markup_kind(&self) -> MarkupKind {
        self.hover_markup_kind
    }

    pub(super) fn supports_diagnostic_related_information(&self) -> bool {
        self.supports_diagnostic_related_information
    }

    pub(super) fn supports_hierarchical_document_symbols(&self) -> bool {
        matches!(
            self.hierarchical_document_symbols,
            HierarchicalDocumentSymbols::Supported
        )
    }

    pub(super) fn open_document_uris(&self) -> impl Iterator<Item = &Uri> {
        self.index.documents().map(TextDocument::uri)
    }

    pub(super) fn replace_published_diagnostic_paths(
        &mut self,
        paths: HashSet<Utf8PathBuf>,
    ) -> HashSet<Utf8PathBuf> {
        std::mem::replace(&mut self.published_diagnostic_paths, paths)
    }

    /// Analyzes one open document against ancestor fixture providers without
    /// walking unrelated workspace files.
    pub(super) fn analyze_open_document_for_diagnostics(
        &mut self,
        uri: &Uri,
    ) -> Result<Option<DiagnosticAnalysisSnapshot>, SessionError> {
        let document = self
            .index
            .document(uri)
            .cloned()
            .ok_or_else(|| SessionError::DocumentNotOpen(uri.clone()))?;
        if document.language_id() != &LanguageKind::Python {
            return Ok(None);
        }

        let path = uri_to_path(uri)?;
        let project = self.workspaces.project_for_uri(uri)?;
        let project_root = project.cwd().clone();
        let settings = SourceAnalysisSettings {
            python_version: project.metadata().python_version(),
            test_function_prefix: project.settings().test().test_function_prefix.clone(),
            try_import_fixtures: project.settings().test().try_import_fixtures,
        };
        let current = SourceDocument::new(path.clone(), document.contents().to_owned());
        let parent_directory = path
            .parent()
            .ok_or_else(|| WorkspaceError::MissingParent(path.clone()))?;
        let mut directories = parent_directory
            .ancestors()
            .filter(|directory| directory.starts_with(&project_root))
            .collect::<Vec<_>>();
        directories.reverse();
        let parent_paths = directories
            .into_iter()
            .map(|directory| directory.join("conftest.py"))
            .filter(|conftest_path| conftest_path != &path);
        let open_sources = self
            .index
            .documents()
            .filter_map(|open_document| {
                uri_to_path(open_document.uri())
                    .ok()
                    .map(|open_path| (open_path, open_document.contents().to_owned()))
            })
            .collect::<HashMap<_, _>>();
        let mut sources = HashMap::from([(path.clone(), document.contents().to_owned())]);
        let mut parents = Vec::new();
        for parent_path in parent_paths {
            let source_text = if let Some(source_text) = open_sources.get(&parent_path) {
                source_text.clone()
            } else {
                if !parent_path.is_file() {
                    continue;
                }
                fs::read_to_string(&parent_path).map_err(|source| SourceIndexError::ReadSource {
                    path: parent_path.clone(),
                    source,
                })?
            };
            sources.insert(parent_path.clone(), source_text.clone());
            parents.push(SourceDocument::new(parent_path, source_text));
        }
        let Some(analysis) =
            analyze_source_with_parents(current, parents, &project_root, &settings)
        else {
            return Ok(None);
        };
        Ok(Some(DiagnosticAnalysisSnapshot { analysis, sources }))
    }

    pub(super) fn open_document(&mut self, document: TextDocument) {
        self.index.open_document(document);
        self.invalidate_source_indexes();
    }

    pub(super) fn document(&self, uri: &Uri) -> Option<&TextDocument> {
        self.index.document(uri)
    }

    pub(super) fn document_uri_for_path(&self, path: &Utf8Path) -> Option<Uri> {
        self.index
            .document_for_path(path)
            .map(|document| document.uri().clone())
    }

    /// Captures open-document and project state without reading or parsing
    /// provider source files.
    pub(super) fn prepare_source_analysis(
        &mut self,
        uri: &Uri,
    ) -> Result<Option<PreparedSourceAnalysis>, SessionError> {
        self.prepare_source_analysis_with_scope(uri, SourceIndexScope::TestSelection)
    }

    /// Captures source state for a project-wide symbol query.
    pub fn prepare_project_source_analysis(
        &mut self,
        uri: &Uri,
    ) -> Result<Option<PreparedSourceAnalysis>, SessionError> {
        self.prepare_source_analysis_with_scope(uri, SourceIndexScope::Project)
    }

    fn prepare_source_analysis_with_scope(
        &mut self,
        uri: &Uri,
        scope: SourceIndexScope,
    ) -> Result<Option<PreparedSourceAnalysis>, SessionError> {
        let document = self
            .index
            .document(uri)
            .cloned()
            .ok_or_else(|| SessionError::DocumentNotOpen(uri.clone()))?;
        if document.language_id() != &LanguageKind::Python {
            return Ok(None);
        }

        let path = uri_to_path(uri)?;
        let project = self.workspaces.project_for_uri(uri)?;
        let project_root = project.cwd().clone();
        let settings = SourceAnalysisSettings {
            python_version: project.metadata().python_version(),
            test_function_prefix: project.settings().test().test_function_prefix.clone(),
            try_import_fixtures: project.settings().test().try_import_fixtures,
        };

        let open_sources = self
            .index
            .documents()
            .filter(|document| document.language_id() == &LanguageKind::Python)
            .filter_map(|open_document| {
                uri_to_path(open_document.uri())
                    .ok()
                    .map(|open_path| (open_path, open_document.contents().to_owned()))
            })
            .collect::<BTreeMap<_, _>>();
        let source_index_cache = if self.cache_source_indexes {
            self.source_indexes
                .entry((project_root.clone(), scope))
                .or_default()
                .clone()
        } else {
            SourceIndexCache::default()
        };
        let source_index = PreparedSourceIndex::with_cache(
            project_root,
            project.settings().src().include_paths.clone(),
            open_sources,
            settings,
            project.settings().src().respect_ignore_files,
            scope,
            source_index_cache,
        );

        Ok(Some(PreparedSourceAnalysis {
            current_path: path,
            current_source: document.contents().to_owned(),
            source_index,
            document_uri: uri.clone(),
            document_version: document.version(),
            source_index_revision: self.source_index_revision.clone(),
        }))
    }

    pub(super) fn configuration_changed(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.workspaces.configuration_changed(uri)?;
        self.invalidate_source_indexes();
        Ok(())
    }

    pub(super) fn update_document(
        &mut self,
        uri: &Uri,
        changes: Vec<TextDocumentContentChangeEvent>,
        version: i32,
    ) -> Result<(), SessionError> {
        self.index
            .update_document(uri, changes, version, self.position_encoding)?;
        self.invalidate_source_indexes();
        Ok(())
    }

    pub(super) fn close_document(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.index.close_document(uri)?;
        self.invalidate_source_indexes();
        Ok(())
    }

    pub(super) fn open_workspace_folder(
        &mut self,
        folder: WorkspaceFolder,
    ) -> Result<(), SessionError> {
        self.workspaces.open_folder(folder.clone())?;
        self.index.open_workspace_folder(folder);
        self.invalidate_source_indexes();
        Ok(())
    }

    pub(super) fn close_workspace_folder(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.workspaces.close_folder(uri)?;
        self.index.close_workspace_folder(uri)?;
        self.invalidate_source_indexes();
        Ok(())
    }

    fn is_source_index_revision_current(&self, revision: &SourceIndexRevision) -> bool {
        Arc::ptr_eq(&self.source_index_revision.0, &revision.0)
    }

    pub(crate) fn enable_source_index_cache(&mut self) {
        self.cache_source_indexes = true;
    }

    pub(crate) fn is_document_snapshot_current(&self, version: &DocumentSnapshotVersion) -> bool {
        self.document(&version.uri)
            .is_some_and(|document| document.version() == version.document_version)
            && self.is_source_index_revision_current(&version.source_index_revision)
    }

    fn invalidate_source_indexes(&mut self) {
        self.source_indexes.clear();
        self.source_index_revision = SourceIndexRevision::default();
    }
}
