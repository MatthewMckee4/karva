//! Mutable language-server state.

pub mod client;
mod index;
mod request_queue;
mod source_index;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use karva_ide::{SourceAnalysis, SourceAnalysisSettings, SourceDiagnostic, WorkspaceSourceIndex};
use lsp_types::{LanguageKind, MarkupKind, TextDocumentContentChangeEvent, Uri, WorkspaceFolder};

use crate::workspace::{PreparedProjectDiscovery, WorkspaceError, Workspaces, uri_to_path};
use crate::{PositionEncoding, TextDocument};

use self::index::Index;
use self::request_queue::RequestQueue;
use self::source_index::{SourceIndexCache, SourceIndexScope};

pub use self::request_queue::RequestCancellationToken;
pub use self::source_index::{PreparedSourceIndex, SourceIndexError};

/// Identity of the project source state captured for a background request.
#[derive(Clone, Debug)]
pub struct SourceIndexRevision(Arc<()>);

impl Default for SourceIndexRevision {
    fn default() -> Self {
        Self(Arc::new(()))
    }
}

/// Document and project state that must remain current for a response.
#[derive(Clone, Debug)]
pub struct DocumentSnapshotVersion {
    pub(crate) uri: Uri,
    pub(crate) document_version: i32,
    pub(crate) source_index_revision: SourceIndexRevision,
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
pub struct SourceAnalysisSnapshot {
    /// Analysis for the current open document.
    pub analysis: SourceAnalysis,

    /// Immutable project source snapshot used by the analysis.
    pub source_index: Arc<WorkspaceSourceIndex>,
}

/// Owned source analysis captured before diagnostic work runs on a worker.
#[derive(Debug)]
pub struct PreparedDiagnostics {
    documents: Vec<PreparedDiagnosticDocument>,
    open_sources: BTreeMap<Utf8PathBuf, String>,
    open_python_paths: HashSet<Utf8PathBuf>,
    source_index_revision: SourceIndexRevision,
    cancellation: RequestCancellationToken,
}

#[derive(Debug)]
struct PreparedDiagnosticDocument {
    path: Utf8PathBuf,
    project: PreparedProjectDiscovery,
}

impl PreparedDiagnostics {
    /// Computes diagnostics without reading files or parsing on the event loop.
    pub fn analyze(self) -> Option<DiagnosticAnalysis> {
        let Self {
            documents,
            open_sources,
            open_python_paths,
            source_index_revision,
            cancellation,
        } = self;
        if cancellation.is_cancelled() {
            return None;
        }
        let mut projects = BTreeMap::<Utf8PathBuf, (PreparedSourceIndex, Vec<Utf8PathBuf>)>::new();
        let mut diagnostics_by_path = BTreeMap::<Utf8PathBuf, Vec<SourceDiagnostic>>::new();
        let mut errors_by_path = BTreeMap::<Utf8PathBuf, Vec<String>>::new();
        let mut sources = HashMap::<Utf8PathBuf, String>::new();

        for document in documents {
            if cancellation.is_cancelled() {
                return None;
            }
            let project = match document.project.discover() {
                Ok(project) => project,
                Err(error) => {
                    errors_by_path
                        .entry(document.path)
                        .or_default()
                        .push(error.to_string());
                    continue;
                }
            };
            if cancellation.is_cancelled() {
                return None;
            }
            let project_root = project.cwd().clone();
            let project_sources = open_sources
                .iter()
                .filter(|(path, _)| path.starts_with(&project_root))
                .map(|(path, source)| (path.clone(), source.clone()))
                .collect();
            let prepared_index = PreparedSourceIndex::with_cache(
                project_root.clone(),
                Vec::new(),
                project_sources,
                SourceAnalysisSettings {
                    python_version: project.metadata().python_version(),
                    test_function_prefix: project.settings().test().test_function_prefix.clone(),
                    try_import_fixtures: project.settings().test().try_import_fixtures,
                },
                project.settings().src().respect_ignore_files,
                SourceIndexScope::OpenDocuments,
                SourceIndexCache::default(),
            );
            projects
                .entry(project_root)
                .or_insert_with(|| (prepared_index, Vec::new()))
                .1
                .push(document.path);
        }

        for (prepared_index, paths) in projects.into_values() {
            if cancellation.is_cancelled() {
                return None;
            }
            let index = match prepared_index.build(&cancellation) {
                Ok(index) => index,
                Err(error) => {
                    if cancellation.is_cancelled() {
                        return None;
                    }
                    for path in paths {
                        errors_by_path
                            .entry(path)
                            .or_default()
                            .push(error.to_string());
                    }
                    continue;
                }
            };
            for current_path in paths {
                if cancellation.is_cancelled() {
                    return None;
                }
                let Some(analysis) = index.analyze(&current_path) else {
                    continue;
                };
                if cancellation.is_cancelled() {
                    return None;
                }
                for diagnostic in analysis.diagnostics {
                    if cancellation.is_cancelled() {
                        return None;
                    }
                    let source_paths = std::iter::once(&diagnostic.location.path).chain(
                        diagnostic
                            .related
                            .iter()
                            .map(|related| &related.location.path),
                    );
                    for path in source_paths {
                        if let Some(module) = index.module(path) {
                            sources
                                .entry(path.clone())
                                .or_insert_with(|| module.source_text.clone());
                        }
                    }
                    diagnostics_by_path
                        .entry(diagnostic.location.path.clone())
                        .or_default()
                        .push(diagnostic);
                }
            }
        }

        Some(DiagnosticAnalysis {
            open_python_paths,
            diagnostics_by_path,
            errors_by_path,
            sources,
            source_index_revision,
            cancellation,
        })
    }
}

/// Raw diagnostic analysis awaiting background protocol conversion.
#[derive(Debug)]
pub struct DiagnosticAnalysis {
    pub(crate) open_python_paths: HashSet<Utf8PathBuf>,
    pub(crate) diagnostics_by_path: BTreeMap<Utf8PathBuf, Vec<SourceDiagnostic>>,
    pub(crate) errors_by_path: BTreeMap<Utf8PathBuf, Vec<String>>,
    pub(crate) sources: HashMap<Utf8PathBuf, String>,
    pub(crate) source_index_revision: SourceIndexRevision,
    pub(crate) cancellation: RequestCancellationToken,
}

#[derive(Clone, Copy, Debug)]
enum HierarchicalDocumentSymbols {
    Supported,
    Unsupported,
}

/// Owned inputs for source analysis that can move off the event-loop thread.
#[derive(Debug)]
pub struct PreparedSourceAnalysis {
    current_path: Utf8PathBuf,
    current_source: String,
    source_index: PreparedSourceIndex,
    document_uri: Uri,
    document_version: i32,
    source_index_revision: SourceIndexRevision,
}

impl PreparedSourceAnalysis {
    pub fn source_text(&self) -> &str {
        &self.current_source
    }

    pub fn response_version(&self) -> DocumentSnapshotVersion {
        DocumentSnapshotVersion {
            uri: self.document_uri.clone(),
            document_version: self.document_version,
            source_index_revision: self.source_index_revision.clone(),
        }
    }

    /// Builds the immutable project source snapshot on a worker thread.
    pub fn into_source_index(
        self,
        cancellation: &RequestCancellationToken,
    ) -> Result<Arc<WorkspaceSourceIndex>, SessionError> {
        Ok(self.source_index.build(cancellation)?)
    }

    /// Reads provider files and computes source semantics away from the event loop.
    pub fn analyze(
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
    diagnostic_cancellation: RequestCancellationToken,
}

impl Session {
    pub fn new(
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
            diagnostic_cancellation: RequestCancellationToken::default(),
        }
    }

    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown_requested
    }

    pub fn request_shutdown(&mut self) {
        self.shutdown_requested = true;
        self.diagnostic_cancellation.cancel();
    }

    pub fn request_queue_mut(&mut self) -> &mut RequestQueue {
        &mut self.request_queue
    }

    pub fn request_queue(&self) -> &RequestQueue {
        &self.request_queue
    }

    pub fn position_encoding(&self) -> PositionEncoding {
        self.position_encoding
    }

    pub fn hover_markup_kind(&self) -> MarkupKind {
        self.hover_markup_kind
    }

    pub fn supports_diagnostic_related_information(&self) -> bool {
        self.supports_diagnostic_related_information
    }

    pub fn supports_hierarchical_document_symbols(&self) -> bool {
        matches!(
            self.hierarchical_document_symbols,
            HierarchicalDocumentSymbols::Supported
        )
    }

    pub fn open_document_uris(&self) -> impl Iterator<Item = &Uri> {
        self.index.documents().map(TextDocument::uri)
    }

    pub fn replace_published_diagnostic_paths(
        &mut self,
        paths: HashSet<Utf8PathBuf>,
    ) -> HashSet<Utf8PathBuf> {
        std::mem::replace(&mut self.published_diagnostic_paths, paths)
    }

    /// Captures all open Python documents for one latest-only diagnostic job.
    pub fn prepare_diagnostics(&mut self) -> PreparedDiagnostics {
        self.diagnostic_cancellation.cancel();
        let cancellation = RequestCancellationToken::default();
        self.diagnostic_cancellation = cancellation.clone();
        let mut documents = Vec::new();
        let mut open_sources = BTreeMap::new();
        let mut open_python_paths = HashSet::new();
        let uris = self.open_document_uris().cloned().collect::<Vec<_>>();
        for uri in uris {
            let Some(document) = self.document(&uri) else {
                continue;
            };
            if document.language_id() != &LanguageKind::Python {
                continue;
            }
            let Ok(path) = uri_to_path(&uri) else {
                continue;
            };
            open_python_paths.insert(path.clone());
            open_sources.insert(path.clone(), document.contents().to_owned());
            if let Ok(project) = self.workspaces.prepare_project_discovery(&uri) {
                documents.push(PreparedDiagnosticDocument { path, project });
            }
        }
        PreparedDiagnostics {
            documents,
            open_sources,
            open_python_paths,
            source_index_revision: self.source_index_revision.clone(),
            cancellation,
        }
    }

    pub fn open_document(&mut self, document: TextDocument) {
        self.index.open_document(document);
        self.invalidate_source_indexes();
    }

    pub fn document(&self, uri: &Uri) -> Option<&TextDocument> {
        self.index.document(uri)
    }

    pub fn document_uri_for_path(&self, path: &Utf8Path) -> Option<Uri> {
        self.index
            .document_for_path(path)
            .map(|document| document.uri().clone())
    }

    /// Captures open-document and project state without reading or parsing
    /// provider source files.
    pub fn prepare_source_analysis(
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

    pub fn configuration_changed(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.workspaces.configuration_changed(uri)?;
        self.invalidate_source_indexes();
        Ok(())
    }

    pub fn update_document(
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

    pub fn close_document(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.index.close_document(uri)?;
        self.invalidate_source_indexes();
        Ok(())
    }

    pub fn open_workspace_folder(&mut self, folder: WorkspaceFolder) -> Result<(), SessionError> {
        self.workspaces.open_folder(folder.clone())?;
        self.index.open_workspace_folder(folder);
        self.invalidate_source_indexes();
        Ok(())
    }

    pub fn close_workspace_folder(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.workspaces.close_folder(uri)?;
        self.index.close_workspace_folder(uri)?;
        self.invalidate_source_indexes();
        Ok(())
    }

    pub fn is_source_index_revision_current(&self, revision: &SourceIndexRevision) -> bool {
        Arc::ptr_eq(&self.source_index_revision.0, &revision.0)
    }

    pub fn enable_source_index_cache(&mut self) {
        self.cache_source_indexes = true;
    }

    pub fn is_document_snapshot_current(&self, version: &DocumentSnapshotVersion) -> bool {
        self.document(&version.uri)
            .is_some_and(|document| document.version() == version.document_version)
            && self.is_source_index_revision_current(&version.source_index_revision)
    }

    fn invalidate_source_indexes(&mut self) {
        self.source_indexes.clear();
        self.source_index_revision = SourceIndexRevision::default();
    }
}

#[cfg(test)]
mod tests {
    use ruff_python_ast::PythonVersion;

    use super::Session;
    use crate::workspace::Workspaces;
    use crate::{PositionEncoding, TextDocument};

    #[test]
    fn shutdown_cancels_prepared_diagnostics() -> anyhow::Result<()> {
        let mut session = Session::new(
            PositionEncoding::UTF16,
            lsp_types::MarkupKind::PlainText,
            false,
            false,
            Workspaces::new(Vec::new(), PythonVersion::PY312, None)?,
        );
        let diagnostics = session.prepare_diagnostics();

        session.request_shutdown();

        assert!(diagnostics.cancellation.is_cancelled());
        assert!(diagnostics.analyze().is_none());
        Ok(())
    }

    #[test]
    fn newer_diagnostics_cancel_and_stale_the_previous_generation() -> anyhow::Result<()> {
        let temporary = tempfile::tempdir()?;
        std::fs::create_dir(temporary.path().join(".git"))?;
        let uri = lsp_types::Uri::from_file_path(temporary.path().join("test_example.py"))
            .map_err(|()| anyhow::anyhow!("temporary document path should produce a URI"))?;
        let mut session = Session::new(
            PositionEncoding::UTF16,
            lsp_types::MarkupKind::PlainText,
            false,
            false,
            Workspaces::new(Vec::new(), PythonVersion::PY312, None)?,
        );
        session.open_document(TextDocument::new(
            uri.clone(),
            "def test_example(missing): pass\n".to_owned(),
            1,
            lsp_types::LanguageKind::Python,
        ));
        let previous = session.prepare_diagnostics();
        let previous_revision = previous.source_index_revision.clone();

        session.update_document(
            &uri,
            vec![
                lsp_types::TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                    lsp_types::TextDocumentContentChangeWholeDocument {
                        text: "def test_example(): pass\n".to_owned(),
                    },
                ),
            ],
            2,
        )?;
        let current = session.prepare_diagnostics();

        assert!(previous.cancellation.is_cancelled());
        assert!(!session.is_source_index_revision_current(&previous_revision));
        assert!(!current.cancellation.is_cancelled());
        Ok(())
    }
}
