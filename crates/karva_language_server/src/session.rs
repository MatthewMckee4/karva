//! Mutable language-server state.

#![expect(
    clippy::redundant_pub_crate,
    reason = "server endpoints consume session APIs across private sibling modules"
)]

pub(super) mod client;
mod index;
mod request_queue;
mod source_index;
mod workspace_symbols;
pub(super) use workspace_symbols::PreparedWorkspaceSymbols;
use workspace_symbols::WorkspaceSymbolCache;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};

use camino::{Utf8Path, Utf8PathBuf};
use karva_ide::{SourceAnalysis, SourceAnalysisSettings, SourceDiagnostic, WorkspaceSourceIndex};
use karva_project::Project;
use lsp_types::{
    InlayHintWorkspaceClientCapabilities, LanguageKind, MarkupKind, TextDocumentContentChangeEvent,
    Uri, WorkspaceEditClientCapabilities, WorkspaceFolder,
};
use once_cell::sync::OnceCell;

use crate::workspace::{WorkspaceError, Workspaces, uri_to_path};
use crate::{PositionEncoding, PreparedProjectDiscovery, TextDocument};

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
    pub(super) uri: Uri,
    document_version: i32,
    source_index_revision: SourceIndexRevision,
    /// Resolved metadata published by the worker only for this source revision.
    project: Arc<OnceCell<Arc<Project>>>,
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

    /// A prior worker panic poisoned the source-index cache registry.
    #[error("language-server source-index cache lock poisoned")]
    CachePoisoned,

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

/// Owned source analysis captured before diagnostic work runs on a worker.
#[derive(Debug)]
pub(super) struct PreparedDiagnostics {
    documents: Vec<PreparedDiagnosticDocument>,
    open_sources: BTreeMap<Utf8PathBuf, Arc<str>>,
    open_python_paths: HashSet<Utf8PathBuf>,
    source_indexes: SharedSourceIndexes,
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
    pub(super) fn analyze(self) -> Option<DiagnosticAnalysis> {
        let Self {
            documents,
            open_sources,
            open_python_paths,
            source_indexes,
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
                        .push(format!("{:#}", anyhow::Error::new(error)));
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
            let cache = match source_index_cache(
                &source_indexes,
                project_root.clone(),
                SourceIndexScope::OpenDocuments,
                false,
            ) {
                Ok(cache) => cache,
                Err(error) => {
                    errors_by_path
                        .entry(document.path)
                        .or_default()
                        .push(error.to_string());
                    continue;
                }
            };
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
                cache,
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
                    let message = format!("{:#}", anyhow::Error::new(error));
                    for path in paths {
                        errors_by_path
                            .entry(path)
                            .or_default()
                            .push(message.clone());
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
pub(super) struct DiagnosticAnalysis {
    pub(super) open_python_paths: HashSet<Utf8PathBuf>,
    pub(super) diagnostics_by_path: BTreeMap<Utf8PathBuf, Vec<SourceDiagnostic>>,
    pub(super) errors_by_path: BTreeMap<Utf8PathBuf, Vec<String>>,
    pub(super) sources: HashMap<Utf8PathBuf, String>,
    pub(super) source_index_revision: SourceIndexRevision,
    pub(super) cancellation: RequestCancellationToken,
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
    current_source: Arc<str>,
    project_discovery: PreparedProjectDiscovery,
    project: Arc<OnceCell<Arc<Project>>>,
    open_sources: BTreeMap<Utf8PathBuf, Arc<str>>,
    scope: SourceIndexScope,
    source_indexes: SharedSourceIndexes,
    retain_source_snapshot: bool,
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
            project: Arc::clone(&self.project),
            source_index_revision: self.source_index_revision.clone(),
        }
    }

    /// Builds the immutable project source snapshot on a worker thread.
    fn into_source_index(
        self,
        cancellation: &RequestCancellationToken,
    ) -> Result<Arc<WorkspaceSourceIndex>, SessionError> {
        if cancellation.is_cancelled() {
            return Err(SourceIndexError::Cancelled.into());
        }
        let project = self.project_discovery.discover()?;
        if cancellation.is_cancelled() {
            return Err(SourceIndexError::Cancelled.into());
        }
        let project_root = project.cwd().clone();
        let cache = source_index_cache(
            &self.source_indexes,
            project_root.clone(),
            self.scope,
            self.retain_source_snapshot,
        )?;
        let index = PreparedSourceIndex::with_cache(
            project_root,
            project.settings().src().include_paths.clone(),
            self.open_sources,
            SourceAnalysisSettings {
                python_version: project.metadata().python_version(),
                test_function_prefix: project.settings().test().test_function_prefix.clone(),
                try_import_fixtures: project.settings().test().try_import_fixtures,
            },
            project.settings().src().respect_ignore_files,
            self.scope,
            cache,
        )
        .build(cancellation)?;
        let _ = self.project.set(project);
        Ok(index)
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

/// Cache registry shared only within one immutable source revision.
/// Discovery happens before locking; no filesystem or analysis work holds the lock.
type SharedSourceIndexes = Arc<Mutex<HashMap<(Utf8PathBuf, SourceIndexScope), SourceIndexCache>>>;

/// Selects a generation cache; unwatched clients still rebuild from disk for every request.
fn source_index_cache(
    caches: &SharedSourceIndexes,
    root: Utf8PathBuf,
    scope: SourceIndexScope,
    retain_snapshot: bool,
) -> Result<SourceIndexCache, SessionError> {
    let mut caches = caches.lock().map_err(|_| SessionError::CachePoisoned)?;
    let cache = caches.entry((root, scope)).or_default();
    Ok(if retain_snapshot {
        Arc::clone(cache)
    } else {
        Arc::new(cache.invalidated())
    })
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
    inlay_hint_refresh: Option<InlayHintWorkspaceClientCapabilities>,
    versioned_code_actions: Option<WorkspaceEditClientCapabilities>,
    workspaces: Workspaces,
    published_diagnostic_paths: HashSet<Utf8PathBuf>,
    cache_source_indexes: bool,
    source_indexes: SharedSourceIndexes,
    workspace_symbol_cache: WorkspaceSymbolCache,
    source_index_revision: SourceIndexRevision,
    diagnostic_cancellation: RequestCancellationToken,
}

impl Session {
    pub(super) fn new(
        position_encoding: PositionEncoding,
        hover_markup_kind: MarkupKind,
        supports_diagnostic_related_information: bool,
        supports_hierarchical_document_symbols: bool,
        inlay_hint_refresh: Option<InlayHintWorkspaceClientCapabilities>,
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
            inlay_hint_refresh,
            versioned_code_actions: None,
            workspaces,
            published_diagnostic_paths: HashSet::new(),
            cache_source_indexes: false,
            source_indexes: Arc::default(),
            workspace_symbol_cache: Arc::default(),
            source_index_revision: SourceIndexRevision::default(),
            diagnostic_cancellation: RequestCancellationToken::default(),
        }
    }

    /// Enables edits only for clients that can reject obsolete document versions.
    pub(super) fn with_versioned_code_actions(
        mut self,
        supported: Option<WorkspaceEditClientCapabilities>,
    ) -> Self {
        self.versioned_code_actions = supported;
        self
    }

    pub(super) fn supports_versioned_code_actions(&self) -> bool {
        self.versioned_code_actions
            .as_ref()
            .and_then(|capability| capability.document_changes)
            .unwrap_or(false)
    }

    pub(super) fn is_shutdown_requested(&self) -> bool {
        self.shutdown_requested
    }

    pub(super) fn request_shutdown(&mut self) {
        self.shutdown_requested = true;
        self.diagnostic_cancellation.cancel();
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

    pub(super) fn supports_inlay_hint_refresh(&self) -> bool {
        self.inlay_hint_refresh
            .as_ref()
            .and_then(|capability| capability.refresh_support)
            .unwrap_or(false)
    }

    fn open_document_uris(&self) -> impl Iterator<Item = &Uri> {
        self.index.documents().map(TextDocument::uri)
    }

    pub(super) fn replace_published_diagnostic_paths(
        &mut self,
        paths: HashSet<Utf8PathBuf>,
    ) -> HashSet<Utf8PathBuf> {
        std::mem::replace(&mut self.published_diagnostic_paths, paths)
    }

    /// Captures all open Python documents for one latest-only diagnostic job.
    pub(super) fn prepare_diagnostics(&mut self) -> PreparedDiagnostics {
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
            open_sources.insert(path.clone(), document.shared_contents());
            if let Ok(project) = self.workspaces.prepare_project_discovery(&uri) {
                documents.push(PreparedDiagnosticDocument { path, project });
            }
        }
        PreparedDiagnostics {
            source_indexes: Arc::clone(&self.source_indexes),
            documents,
            open_sources,
            open_python_paths,
            source_index_revision: self.source_index_revision.clone(),
            cancellation,
        }
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
        &self,
        uri: &Uri,
    ) -> Result<Option<PreparedSourceAnalysis>, SessionError> {
        self.prepare_source_analysis_with_scope(uri, SourceIndexScope::TestSelection)
    }

    /// Profile passed to editor-run commands to preserve initialization overrides.
    pub(super) fn configuration_profile(&self) -> Option<&str> {
        self.workspaces.profile()
    }

    /// Captures workspace roots and Python overlays without filesystem I/O.
    pub(super) fn prepare_workspace_symbols(&self) -> PreparedWorkspaceSymbols {
        PreparedWorkspaceSymbols::new(
            self.workspaces.clone(),
            self.index
                .documents()
                .filter(|document| document.language_id() == &LanguageKind::Python)
                .filter_map(|document| {
                    uri_to_path(document.uri())
                        .ok()
                        .map(|path| (path, document.shared_contents()))
                })
                .collect(),
            if self.cache_source_indexes {
                Arc::clone(&self.workspace_symbol_cache)
            } else {
                Arc::default()
            },
        )
    }

    /// Captures source state for a project-wide symbol query.
    pub(super) fn prepare_project_source_analysis(
        &self,
        uri: &Uri,
    ) -> Result<Option<PreparedSourceAnalysis>, SessionError> {
        self.prepare_source_analysis_with_scope(uri, SourceIndexScope::Project)
    }

    fn prepare_source_analysis_with_scope(
        &self,
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
        let project_discovery = self.workspaces.prepare_project_discovery(uri)?;

        let open_sources = self
            .index
            .documents()
            .filter(|document| document.language_id() == &LanguageKind::Python)
            .filter_map(|open_document| {
                uri_to_path(open_document.uri())
                    .ok()
                    .map(|open_path| (open_path, open_document.shared_contents()))
            })
            .collect::<BTreeMap<_, _>>();
        Ok(Some(PreparedSourceAnalysis {
            current_path: path,
            current_source: document.shared_contents(),
            project_discovery,
            project: Arc::default(),
            open_sources,
            scope,
            source_indexes: Arc::clone(&self.source_indexes),
            retain_source_snapshot: self.cache_source_indexes,
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
        let folder = uri_to_path(uri)?;
        self.source_indexes
            .lock()
            .map_err(|_| SessionError::CachePoisoned)?
            .retain(|(root, _), _| !root.starts_with(&folder));
        self.invalidate_source_indexes();
        Ok(())
    }

    pub(super) fn is_source_index_revision_current(&self, revision: &SourceIndexRevision) -> bool {
        Arc::ptr_eq(&self.source_index_revision.0, &revision.0)
    }

    /// Retains worker-resolved metadata only while its request snapshot is current.
    pub(super) fn retain_project_metadata(&mut self, version: &DocumentSnapshotVersion) {
        if self.cache_source_indexes
            && self.is_document_snapshot_current(version)
            && let Some(project) = version.project.get()
        {
            self.workspaces
                .retain_project(&version.uri, Arc::clone(project));
        }
    }

    pub(super) fn enable_source_index_cache(&mut self) {
        self.cache_source_indexes = true;
    }

    pub(super) fn is_document_snapshot_current(&self, version: &DocumentSnapshotVersion) -> bool {
        self.document(&version.uri)
            .is_some_and(|document| document.version() == version.document_version)
            && self.is_source_index_revision_current(&version.source_index_revision)
    }

    fn invalidate_source_indexes(&mut self) {
        self.workspace_symbol_cache = Arc::default();
        let next = self
            .source_indexes
            .lock()
            .map(|caches| {
                caches
                    .iter()
                    .map(|(key, cache)| (key.clone(), Arc::new(cache.invalidated())))
                    .collect()
            })
            .unwrap_or_default();
        self.source_indexes = Arc::new(Mutex::new(next));
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
    fn cold_project_configuration_is_read_only_by_worker() -> anyhow::Result<()> {
        let temporary = tempfile::tempdir()?;
        let config = temporary.path().join("karva.toml");
        std::fs::write(&config, "invalid TOML [")?;
        let uri = lsp_types::Uri::from_file_path(temporary.path().join("test_example.py"))
            .map_err(|()| anyhow::anyhow!("temporary file URI"))?;
        let mut session = Session::new(
            PositionEncoding::UTF16,
            lsp_types::MarkupKind::PlainText,
            false,
            false,
            None,
            Workspaces::new(Vec::new(), PythonVersion::PY312, None)?,
        );
        session.open_document(TextDocument::new(
            uri.clone(),
            "def test_example(): pass\n".to_owned(),
            1,
            lsp_types::LanguageKind::Python,
        ));
        let prepared = session
            .prepare_source_analysis(&uri)?
            .expect("Python request");
        let cancellation = super::RequestCancellationToken::default();
        cancellation.cancel();
        assert!(matches!(
            prepared.analyze(&cancellation),
            Err(super::SessionError::SourceIndex(
                super::SourceIndexError::Cancelled
            ))
        ));
        let prepared = session
            .prepare_source_analysis(&uri)?
            .expect("Python request");
        // An edit still applies before discovery, even with malformed cold configuration.
        session.update_document(&uri, Vec::new(), 2)?;
        assert!(matches!(
            prepared.analyze(&super::RequestCancellationToken::default()),
            Err(super::SessionError::Workspace(_))
        ));
        // Discovery reads the worker-time configuration, not the preparation-time file.
        let prepared = session
            .prepare_source_analysis(&uri)?
            .expect("Python request");
        let version = prepared.response_version();
        std::fs::write(&config, "")?;
        assert!(
            prepared
                .analyze(&super::RequestCancellationToken::default())?
                .is_some()
        );
        assert!(version.project.get().is_some());
        session.update_document(&uri, Vec::new(), 3)?;
        assert!(!session.is_document_snapshot_current(&version));
        session.retain_project_metadata(&version);
        Ok(())
    }

    #[test]
    fn request_snapshots_retain_one_text_allocation_per_document() -> anyhow::Result<()> {
        let temporary = tempfile::tempdir()?;
        let mut session = Session::new(
            PositionEncoding::UTF16,
            lsp_types::MarkupKind::PlainText,
            false,
            false,
            None,
            Workspaces::new(Vec::new(), PythonVersion::PY312, None)?,
        );
        for name in ["test_first.py", "test_second.py"] {
            let uri = lsp_types::Uri::from_file_path(temporary.path().join(name))
                .map_err(|()| anyhow::anyhow!("temporary file URI"))?;
            session.open_document(TextDocument::new(
                uri,
                "def test_example(): pass\n".repeat(1024),
                1,
                lsp_types::LanguageKind::Python,
            ));
        }
        let uri = session
            .open_document_uris()
            .next()
            .expect("open document")
            .clone();
        let first = session
            .prepare_source_analysis(&uri)?
            .expect("Python request");
        let second = session
            .prepare_source_analysis(&uri)?
            .expect("Python request");
        for (path, source) in &first.open_sources {
            assert!(std::sync::Arc::ptr_eq(source, &second.open_sources[path]));
        }
        assert!(std::sync::Arc::ptr_eq(
            &first.current_source,
            &second.current_source
        ));
        Ok(())
    }

    #[test]
    fn shutdown_cancels_prepared_diagnostics() -> anyhow::Result<()> {
        let mut session = Session::new(
            PositionEncoding::UTF16,
            lsp_types::MarkupKind::PlainText,
            false,
            false,
            None,
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
            None,
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
