//! Mutable language-server state.

pub mod client;
mod index;
mod request_queue;

use std::sync::Arc;

use karva_project::Project;
use lsp_types::{TextDocumentContentChangeEvent, Uri, WorkspaceFolder};

use crate::workspace::{WorkspaceError, Workspaces};
use crate::{PositionEncoding, TextDocument};

use self::index::Index;
use self::request_queue::RequestQueue;

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
}

/// Mutable state owned by the language-server event loop.
#[derive(Debug)]
pub struct Session {
    index: Index,
    position_encoding: PositionEncoding,
    shutdown_requested: bool,
    request_queue: RequestQueue,
    workspaces: Workspaces,
}

impl Session {
    pub fn new(position_encoding: PositionEncoding, workspaces: Workspaces) -> Self {
        Self {
            index: Index::new(workspaces.folders().cloned()),
            position_encoding,
            request_queue: RequestQueue::default(),
            shutdown_requested: false,
            workspaces,
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

    pub fn open_document(&mut self, document: TextDocument) {
        self.index.open_document(document);
    }

    pub fn project_for_uri(&mut self, uri: &Uri) -> Result<Arc<Project>, SessionError> {
        Ok(self.workspaces.project_for_uri(uri)?)
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
