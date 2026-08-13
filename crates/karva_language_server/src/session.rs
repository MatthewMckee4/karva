//! Mutable language-server state.

mod index;

use lsp_types::{TextDocumentContentChangeEvent, Uri, WorkspaceFolder};

use crate::{PositionEncoding, TextDocument};

use self::index::Index;

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
}

/// Mutable state owned by the language-server event loop.
#[derive(Debug)]
pub struct Session {
    index: Index,
    position_encoding: PositionEncoding,
    shutdown_requested: bool,
}

impl Session {
    pub(super) fn new(
        position_encoding: PositionEncoding,
        workspace_folders: impl IntoIterator<Item = WorkspaceFolder>,
    ) -> Self {
        Self {
            index: Index::new(workspace_folders),
            position_encoding,
            shutdown_requested: false,
        }
    }

    pub(super) fn is_shutdown_requested(&self) -> bool {
        self.shutdown_requested
    }

    pub(super) fn request_shutdown(&mut self) {
        self.shutdown_requested = true;
    }

    pub(super) fn open_document(&mut self, document: TextDocument) {
        self.index.open_document(document);
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

    pub(super) fn open_workspace_folder(&mut self, folder: WorkspaceFolder) {
        self.index.open_workspace_folder(folder);
    }

    pub(super) fn close_workspace_folder(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.index.close_workspace_folder(uri)
    }
}
