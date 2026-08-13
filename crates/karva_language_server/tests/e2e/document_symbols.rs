use std::fs;

use insta::assert_json_snapshot;
use lsp_types::{
    ClientCapabilities, DidOpenTextDocumentNotification, DidOpenTextDocumentParams,
    DocumentSymbolClientCapabilities, DocumentSymbolParams, DocumentSymbolRequest, LanguageKind,
    PartialResultParams, PublishDiagnosticsNotification, TextDocumentClientCapabilities,
    TextDocumentIdentifier, TextDocumentItem, Uri, WorkDoneProgressParams, WorkspaceFolder,
};
use tempfile::TempDir;

use super::TestServer;

const SOURCE: &str = concat!(
    "import pytest\n",
    "from karva import fixture\n\n",
    "@fixture(name=\"данные\")\n",
    "def provider(): pass\n\n",
    "@pytest.mark.parametrize(\"value\", [1, 2])\n",
    "def test_example(value): pass\n",
);

#[test]
fn returns_hierarchical_symbols_for_unsaved_karva_source() {
    let workspace = Workspace::new();
    let capabilities = ClientCapabilities {
        text_document: Some(TextDocumentClientCapabilities {
            document_symbol: Some(DocumentSymbolClientCapabilities {
                hierarchical_document_symbol_support: Some(true),
                ..DocumentSymbolClientCapabilities::default()
            }),
            ..TextDocumentClientCapabilities::default()
        }),
        ..ClientCapabilities::default()
    };
    let mut server = TestServer::with_workspace(capabilities, workspace.folder());
    let uri = workspace.uri("test_symbols.py");
    open(&server, uri.clone());

    let response = server.request::<DocumentSymbolRequest>(params(uri));

    assert_json_snapshot!(response);
}

#[test]
fn falls_back_to_flat_symbols_for_legacy_clients() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_symbols.py");
    open(&server, uri.clone());

    let response = server.request::<DocumentSymbolRequest>(params(uri));

    let mut response = serde_json::to_value(response).expect("symbols should serialize");
    normalize_uri(&mut response, workspace.root_uri.as_str());
    assert_json_snapshot!(response);
}

fn open(server: &TestServer, uri: Uri) {
    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri,
            language_id: LanguageKind::Python,
            version: 1,
            text: SOURCE.to_owned(),
        },
    });
    server.receive_notification::<PublishDiagnosticsNotification>();
}

fn params(uri: Uri) -> DocumentSymbolParams {
    DocumentSymbolParams::new(
        TextDocumentIdentifier::new(uri),
        WorkDoneProgressParams::default(),
        PartialResultParams::default(),
    )
}

fn normalize_uri(value: &mut serde_json::Value, workspace_uri: &str) {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                normalize_uri(value, workspace_uri);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                normalize_uri(value, workspace_uri);
            }
        }
        serde_json::Value::String(value) => {
            *value = value.replace(workspace_uri, "file:///project");
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
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
}
