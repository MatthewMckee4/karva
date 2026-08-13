//! Mutable language-server state.

#![expect(
    clippy::redundant_pub_crate,
    reason = "server endpoints consume session APIs across private sibling modules"
)]

pub(super) mod client;
mod index;
mod request_queue;

use std::collections::{HashMap, HashSet};
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use karva_ide::{
    SourceAnalysis, SourceAnalysisSettings, SourceDocument, analyze_source_with_parents,
};
use lsp_types::{LanguageKind, TextDocumentContentChangeEvent, Uri, WorkspaceFolder};

use crate::workspace::{WorkspaceError, Workspaces, uri_to_path};
use crate::{PositionEncoding, TextDocument};

use self::index::Index;
use self::request_queue::RequestQueue;

pub use self::request_queue::RequestCancellationToken;

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

    /// A source document required for analysis could not be read.
    #[error("failed to read Python source file `{path}`: {source}")]
    SourceRead {
        /// File that could not be read.
        path: Utf8PathBuf,

        /// Underlying filesystem failure.
        #[source]
        source: std::io::Error,
    },
}

/// Source analysis and the text needed to map every result back to an editor.
#[derive(Debug)]
pub(super) struct SourceAnalysisSnapshot {
    /// Analysis for the current open document.
    pub(super) analysis: SourceAnalysis,

    /// Source text keyed by every analyzed document path.
    pub(super) sources: HashMap<Utf8PathBuf, String>,
}

/// Owned inputs for source analysis that can move off the event-loop thread.
#[derive(Debug)]
pub(super) struct PreparedSourceAnalysis {
    current: SourceDocument,
    parent_paths: Vec<Utf8PathBuf>,
    open_sources: HashMap<Utf8PathBuf, String>,
    project_root: Utf8PathBuf,
    settings: SourceAnalysisSettings,
    document_uri: Uri,
    document_version: i32,
}

impl PreparedSourceAnalysis {
    pub(super) fn source_text(&self) -> &str {
        self.current.source_text()
    }

    pub(super) fn document_uri(&self) -> &Uri {
        &self.document_uri
    }

    pub(super) fn document_version(&self) -> i32 {
        self.document_version
    }

    /// Reads provider files and computes source semantics away from the event loop.
    pub(super) fn analyze(self) -> Result<Option<SourceAnalysisSnapshot>, SessionError> {
        let current_path = self.current.path().to_path_buf();
        let current_source = self.current.source_text().to_owned();
        let mut sources = HashMap::from([(current_path, current_source)]);
        let mut parents = Vec::new();

        for path in self.parent_paths {
            let source_text = if let Some(source_text) = self.open_sources.get(&path) {
                source_text.clone()
            } else {
                if !path.is_file() {
                    continue;
                }
                fs::read_to_string(&path).map_err(|source| SessionError::SourceRead {
                    path: path.clone(),
                    source,
                })?
            };
            sources.insert(path.clone(), source_text.clone());
            parents.push(SourceDocument::new(path, source_text));
        }

        let Some(analysis) =
            analyze_source_with_parents(self.current, parents, &self.project_root, &self.settings)
        else {
            return Ok(None);
        };
        Ok(Some(SourceAnalysisSnapshot { analysis, sources }))
    }
}

/// Mutable state owned by the language-server event loop.
#[derive(Debug)]
pub struct Session {
    index: Index,
    position_encoding: PositionEncoding,
    shutdown_requested: bool,
    request_queue: RequestQueue,
    supports_diagnostic_related_information: bool,
    workspaces: Workspaces,
    published_diagnostic_paths: HashSet<Utf8PathBuf>,
}

impl Session {
    pub(super) fn new(
        position_encoding: PositionEncoding,
        supports_diagnostic_related_information: bool,
        workspaces: Workspaces,
    ) -> Self {
        Self {
            index: Index::new(workspaces.folders().cloned()),
            position_encoding,
            request_queue: RequestQueue::default(),
            shutdown_requested: false,
            supports_diagnostic_related_information,
            workspaces,
            published_diagnostic_paths: HashSet::new(),
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

    pub(super) fn supports_diagnostic_related_information(&self) -> bool {
        self.supports_diagnostic_related_information
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

    pub(super) fn open_document(&mut self, document: TextDocument) {
        self.index.open_document(document);
    }

    pub(super) fn document(&self, uri: &Uri) -> Option<&TextDocument> {
        self.index.document(uri)
    }

    pub(super) fn document_uri_for_path(&self, path: &Utf8Path) -> Option<Uri> {
        self.index
            .document_for_path(path)
            .map(|document| document.uri().clone())
    }

    /// Analyzes an open Python document with its current editor contents and
    /// visible ancestor `conftest.py` providers.
    pub(super) fn analyze_open_document(
        &mut self,
        uri: &Uri,
    ) -> Result<Option<SourceAnalysisSnapshot>, SessionError> {
        let Some(prepared) = self.prepare_source_analysis(uri)? else {
            return Ok(None);
        };
        prepared.analyze()
    }

    /// Captures open-document and project state without reading or parsing
    /// provider source files.
    pub(super) fn prepare_source_analysis(
        &mut self,
        uri: &Uri,
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
            .filter(|conftest_path| conftest_path != &path)
            .collect();
        let open_sources = self
            .index
            .documents()
            .filter_map(|document| {
                uri_to_path(document.uri())
                    .ok()
                    .map(|path| (path, document.contents().to_owned()))
            })
            .collect();

        Ok(Some(PreparedSourceAnalysis {
            current,
            parent_paths,
            open_sources,
            project_root,
            settings,
            document_uri: uri.clone(),
            document_version: document.version(),
        }))
    }

    pub(super) fn configuration_changed(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.workspaces.configuration_changed(uri)?;
        Ok(())
    }

    pub(super) fn update_document(
        &mut self,
        uri: &Uri,
        changes: Vec<TextDocumentContentChangeEvent>,
        version: i32,
    ) -> Result<(), SessionError> {
        self.index
            .update_document(uri, changes, version, self.position_encoding)
    }

    pub(super) fn close_document(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.index.close_document(uri)
    }

    pub(super) fn open_workspace_folder(
        &mut self,
        folder: WorkspaceFolder,
    ) -> Result<(), SessionError> {
        self.workspaces.open_folder(folder.clone())?;
        self.index.open_workspace_folder(folder);
        Ok(())
    }

    pub(super) fn close_workspace_folder(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.workspaces.close_folder(uri)?;
        self.index.close_workspace_folder(uri)
    }
}
