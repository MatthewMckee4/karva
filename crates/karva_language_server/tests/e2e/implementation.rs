use std::fs;

use insta::assert_json_snapshot;
use lsp_types::{
    ClientCapabilities, DidChangeTextDocumentNotification, DidChangeTextDocumentParams,
    DidOpenTextDocumentNotification, DidOpenTextDocumentParams, ImplementationParams,
    ImplementationRequest, LanguageKind, PartialResultParams, Position,
    PublishDiagnosticsNotification, TextDocumentContentChangeEvent,
    TextDocumentContentChangeWholeDocument, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, VersionedTextDocumentIdentifier, WorkDoneProgressParams,
    WorkspaceFolder,
};
use serde_json::Value;
use tempfile::TempDir;

use super::TestServer;

#[test]
fn navigates_to_generator_and_return_fixture_implementations() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture\ndef database():\n    yield object()\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    let source = concat!(
        "import pytest\n",
        "from karva import fixture\n\n",
        "@fixture\n",
        "def local(): return object()\n\n",
        "@pytest.mark.usefixtures(\"database\")\n",
        "def test_example(database, local, tmp_path): pass\n",
    );
    open(&server, uri.clone(), source);

    let parameter =
        server.request::<ImplementationRequest>(params(uri.clone(), Position::new(7, 21)));
    let metadata =
        server.request::<ImplementationRequest>(params(uri.clone(), Position::new(6, 30)));
    let local = server.request::<ImplementationRequest>(params(uri.clone(), Position::new(7, 31)));
    let builtin = server.request::<ImplementationRequest>(params(uri, Position::new(7, 39)));

    assert_json_snapshot!(workspace.normalize([parameter, metadata, local, builtin]));
}

#[test]
fn rejects_an_implementation_from_a_stale_document_version() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture\ndef database():\n    yield object()\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    open(&server, uri.clone(), "def test_example(database): pass\n");

    let id =
        server.send_request::<ImplementationRequest>(params(uri.clone(), Position::new(0, 20)));
    server.notify::<DidChangeTextDocumentNotification>(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            text_document_identifier: TextDocumentIdentifier::new(uri),
            version: 2,
        },
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument {
                    text: "def test_example(other): pass\n".to_owned(),
                },
            ),
        ],
    });

    let response = server.receive_response(&id);
    let error = response
        .response_result
        .expect_err("stale implementation should fail");
    assert_eq!(error.code, lsp_server::ErrorCode::ContentModified as i32);
}

fn open(server: &TestServer, uri: Uri, source: &str) {
    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri,
            language_id: LanguageKind::Python,
            version: 1,
            text: source.to_owned(),
        },
    });
    server.receive_notification::<PublishDiagnosticsNotification>();
}

fn params(uri: Uri, position: Position) -> ImplementationParams {
    ImplementationParams::new(
        WorkDoneProgressParams::default(),
        PartialResultParams::default(),
        TextDocumentPositionParams::new(TextDocumentIdentifier::new(uri), position),
    )
}

struct Workspace {
    directory: TempDir,
    root_uri: Uri,
}

impl Workspace {
    fn new() -> Self {
        let directory = tempfile::tempdir().expect("temporary workspace should be created");
        fs::create_dir(directory.path().join(".git")).expect("workspace marker should be created");
        let root_uri =
            Uri::from_file_path(directory.path()).expect("workspace URI should be valid");
        Self {
            directory,
            root_uri,
        }
    }

    fn folder(&self) -> WorkspaceFolder {
        WorkspaceFolder {
            uri: self.root_uri.clone(),
            name: "project".to_owned(),
        }
    }

    fn uri(&self, relative: &str) -> Uri {
        Uri::from_file_path(self.directory.path().join(relative))
            .expect("document URI should be valid")
    }

    fn write(&self, relative: &str, source: &str) {
        fs::write(self.directory.path().join(relative), source)
            .expect("workspace source should be written");
    }

    fn normalize(&self, value: impl serde::Serialize) -> Value {
        let mut value = serde_json::to_value(value).expect("implementations should serialize");
        normalize_paths(
            &mut value,
            self.root_uri.as_str(),
            &self.directory.path().to_string_lossy(),
        );
        value
    }
}

fn normalize_paths(value: &mut Value, workspace_uri: &str, workspace_path: &str) {
    match value {
        Value::Array(values) => {
            for value in values {
                normalize_paths(value, workspace_uri, workspace_path);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                normalize_paths(value, workspace_uri, workspace_path);
            }
        }
        Value::String(value) => {
            *value = value
                .replace(workspace_uri, "file:///project")
                .replace(workspace_path, "/project");
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
