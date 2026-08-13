//! Mutable language-server state.

pub mod client;
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
pub struct SourceAnalysisSnapshot {
    /// Analysis for the current open document.
    pub analysis: SourceAnalysis,

    /// Source text keyed by every analyzed document path.
    pub sources: HashMap<Utf8PathBuf, String>,
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
    pub fn new(
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

    pub fn is_shutdown_requested(&self) -> bool {
        self.shutdown_requested
    }

    pub fn request_shutdown(&mut self) {
        self.shutdown_requested = true;
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

    pub fn supports_diagnostic_related_information(&self) -> bool {
        self.supports_diagnostic_related_information
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

    pub fn open_document(&mut self, document: TextDocument) {
        self.index.open_document(document);
    }

    pub fn document(&self, uri: &Uri) -> Option<&TextDocument> {
        self.index.document(uri)
    }

    pub fn document_uri_for_path(&self, path: &Utf8Path) -> Option<Uri> {
        self.index
            .document_for_path(path)
            .map(|document| document.uri().clone())
    }

    /// Analyzes an open Python document with its current editor contents and
    /// visible ancestor `conftest.py` providers.
    pub fn analyze_open_document(
        &mut self,
        uri: &Uri,
    ) -> Result<Option<SourceAnalysisSnapshot>, SessionError> {
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
        let mut parents = Vec::new();
        let mut source_texts = HashMap::from([(path.clone(), document.contents().to_owned())]);
        let parent_directory = path
            .parent()
            .ok_or_else(|| WorkspaceError::MissingParent(path.clone()))?;
        let mut directories = parent_directory
            .ancestors()
            .filter(|directory| directory.starts_with(&project_root))
            .collect::<Vec<_>>();
        directories.reverse();

        for directory in directories {
            let conftest_path = directory.join("conftest.py");
            if conftest_path == path {
                continue;
            }
            let Some(source_text) = self.source_text_for_path(&conftest_path)? else {
                continue;
            };
            source_texts.insert(conftest_path.clone(), source_text.clone());
            parents.push(SourceDocument::new(conftest_path, source_text));
        }

        let Some(analysis) =
            analyze_source_with_parents(current, parents, &project_root, &settings)
        else {
            return Ok(None);
        };

        Ok(Some(SourceAnalysisSnapshot {
            analysis,
            sources: source_texts,
        }))
    }

    pub fn configuration_changed(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.workspaces.configuration_changed(uri)?;
        Ok(())
    }

    pub fn update_document(
        &mut self,
        uri: &Uri,
        changes: Vec<TextDocumentContentChangeEvent>,
        version: i32,
    ) -> Result<(), SessionError> {
        self.index
            .update_document(uri, changes, version, self.position_encoding)
    }

    pub fn close_document(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.index.close_document(uri)
    }

    fn source_text_for_path(&self, path: &Utf8Path) -> Result<Option<String>, SessionError> {
        if let Some(document) = self.index.document_for_path(path) {
            return Ok(Some(document.contents().to_owned()));
        }
        if !path.is_file() {
            return Ok(None);
        }
        fs::read_to_string(path)
            .map(Some)
            .map_err(|source| SessionError::SourceRead {
                path: path.to_path_buf(),
                source,
            })
    }

    pub fn open_workspace_folder(&mut self, folder: WorkspaceFolder) -> Result<(), SessionError> {
        self.workspaces.open_folder(folder.clone())?;
        self.index.open_workspace_folder(folder);
        Ok(())
    }

    pub fn close_workspace_folder(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.workspaces.close_folder(uri)?;
        self.index.close_workspace_folder(uri)
    }
}
