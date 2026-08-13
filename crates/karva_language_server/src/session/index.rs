use std::collections::HashMap;

use lsp_types::{TextDocumentContentChangeEvent, Uri, WorkspaceFolder};

use crate::session::SessionError;
use crate::{PositionEncoding, TextDocument};

/// Open documents and workspace folders indexed by client URI.
#[derive(Debug)]
pub struct Index {
    documents: HashMap<Uri, TextDocument>,
    workspace_folders: HashMap<Uri, WorkspaceFolder>,
}

impl Index {
    pub fn new(workspace_folders: impl IntoIterator<Item = WorkspaceFolder>) -> Self {
        Self {
            documents: HashMap::new(),
            workspace_folders: workspace_folders
                .into_iter()
                .map(|folder| (folder.uri.clone(), folder))
                .collect(),
        }
    }

    pub fn open_document(&mut self, document: TextDocument) {
        self.documents.insert(document.uri().clone(), document);
    }

    pub fn update_document(
        &mut self,
        uri: &Uri,
        changes: Vec<TextDocumentContentChangeEvent>,
        version: i32,
        encoding: PositionEncoding,
    ) -> Result<(), SessionError> {
        let document = self
            .documents
            .get_mut(uri)
            .ok_or_else(|| SessionError::DocumentNotOpen(uri.clone()))?;
        document.apply_changes(changes, version, encoding)?;
        Ok(())
    }

    pub fn close_document(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.documents
            .remove(uri)
            .map(|_| ())
            .ok_or_else(|| SessionError::DocumentNotOpen(uri.clone()))
    }

    pub fn open_workspace_folder(&mut self, folder: WorkspaceFolder) {
        self.workspace_folders.insert(folder.uri.clone(), folder);
    }

    pub fn close_workspace_folder(&mut self, uri: &Uri) -> Result<(), SessionError> {
        self.workspace_folders
            .remove(uri)
            .map(|_| ())
            .ok_or_else(|| SessionError::WorkspaceNotOpen(uri.clone()))
    }
}

#[cfg(test)]
mod tests {
    use lsp_types::{LanguageKind, Uri, WorkspaceFolder};

    use super::*;

    fn uri(value: &str) -> Uri {
        Uri::parse(value).expect("test URI should be valid")
    }

    #[test]
    fn tracks_multiple_workspace_folders() {
        let first = WorkspaceFolder {
            uri: uri("file:///first"),
            name: "first".to_owned(),
        };
        let second = WorkspaceFolder {
            uri: uri("file:///second"),
            name: "second".to_owned(),
        };
        let mut index = Index::new([first.clone(), second.clone()]);

        index
            .close_workspace_folder(&first.uri)
            .expect("first workspace should close");
        index
            .close_workspace_folder(&second.uri)
            .expect("second workspace should close");

        assert!(index.workspace_folders.is_empty());
    }

    #[test]
    fn closes_open_document() {
        let document_uri = uri("file:///test.py");
        let mut index = Index::new([]);
        index.open_document(TextDocument::new(
            document_uri.clone(),
            "def test_example(): pass".to_owned(),
            1,
            LanguageKind::Python,
        ));

        index
            .close_document(&document_uri)
            .expect("open document should close");

        assert!(index.documents.is_empty());
    }
}
