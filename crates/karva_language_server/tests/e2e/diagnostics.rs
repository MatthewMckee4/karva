use std::fs;

use lsp_types::{
    ClientCapabilities, DiagnosticsCapabilities, DidChangeTextDocumentNotification,
    DidChangeTextDocumentParams, DidCloseTextDocumentNotification, DidCloseTextDocumentParams,
    DidOpenTextDocumentNotification, DidOpenTextDocumentParams, LanguageKind,
    PublishDiagnosticsClientCapabilities, PublishDiagnosticsNotification,
    TextDocumentClientCapabilities, TextDocumentContentChangeEvent,
    TextDocumentContentChangeWholeDocument, TextDocumentIdentifier, TextDocumentItem, Uri,
    VersionedTextDocumentIdentifier, WorkspaceFolder,
};
use serde_json::Value;
use tempfile::TempDir;

use super::TestServer;

#[test]
fn publishes_updates_and_clears_unsaved_diagnostics() {
    let workspace = Workspace::new();
    let server = TestServer::with_workspace(capabilities(), workspace.folder());
    let uri = workspace.uri("test_example.py");

    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: LanguageKind::Python,
            version: 1,
            text: "def test_example(database): pass\n".to_owned(),
        },
    });
    let opened = server.receive_notification::<PublishDiagnosticsNotification>();

    server.notify::<DidChangeTextDocumentNotification>(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            text_document_identifier: TextDocumentIdentifier { uri: uri.clone() },
            version: 2,
        },
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument {
                    text: "def test_example(tmp_path): pass\n".to_owned(),
                },
            ),
        ],
    });
    let changed = server.receive_notification::<PublishDiagnosticsNotification>();

    server.notify::<DidCloseTextDocumentNotification>(DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
    });
    let closed = server.receive_notification::<PublishDiagnosticsNotification>();

    insta::assert_json_snapshot!(workspace.normalize([opened, changed, closed]));
}

#[test]
fn publishes_cross_file_related_information() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n\n@fixture\ndef database(): pass\n",
    );
    let server = TestServer::with_workspace(capabilities(), workspace.folder());
    let uri = workspace.uri("test_example.py");

    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri,
            language_id: LanguageKind::Python,
            version: 1,
            text: concat!(
                "from karva import fixture\n\n",
                "@fixture(scope=\"session\")\n",
                "def shared(database): pass\n",
            )
            .to_owned(),
        },
    });
    let diagnostics = server.receive_notification::<PublishDiagnosticsNotification>();

    insta::assert_json_snapshot!(workspace.normalize(diagnostics));
}

#[test]
fn publishes_related_information_for_the_nearest_nested_fixture() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n\n@fixture\ndef database(): pass\n",
    );
    workspace.write(
        "package/conftest.py",
        "from karva import fixture\n\n@fixture\ndef database(): pass\n",
    );
    let server = TestServer::with_workspace(capabilities(), workspace.folder());
    let uri = workspace.uri("package/test_example.py");

    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri,
            language_id: LanguageKind::Python,
            version: 1,
            text: concat!(
                "from karva import fixture\n\n",
                "@fixture(scope=\"session\")\n",
                "def shared(database): pass\n",
            )
            .to_owned(),
        },
    });
    let diagnostics = server.receive_notification::<PublishDiagnosticsNotification>();
    let diagnostic = diagnostics
        .diagnostics
        .first()
        .expect("nearest fixture should produce a scope diagnostic");
    let related = diagnostic
        .related_information
        .as_ref()
        .expect("scope diagnostic should include related information");
    let database = related
        .iter()
        .find(|information| information.message.contains("Fixture `database`"))
        .expect("scope diagnostic should identify the database fixture");

    assert_eq!(database.location.uri, workspace.uri("package/conftest.py"));
}

#[test]
fn does_not_analyze_an_open_conftest_as_its_own_parent() {
    let workspace = Workspace::new();
    let server = TestServer::with_workspace(capabilities(), workspace.folder());
    let uri = workspace.uri("conftest.py");

    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri,
            language_id: LanguageKind::Python,
            version: 1,
            text: "from karva import fixture\n\n@fixture\ndef database(): pass\n".to_owned(),
        },
    });
    let diagnostics = server.receive_notification::<PublishDiagnosticsNotification>();

    insta::assert_json_snapshot!(workspace.normalize(diagnostics));
}

fn capabilities() -> ClientCapabilities {
    ClientCapabilities {
        text_document: Some(TextDocumentClientCapabilities {
            publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                version_support: Some(true),
                diagnostics_capabilities: DiagnosticsCapabilities {
                    related_information: Some(true),
                    ..DiagnosticsCapabilities::default()
                },
            }),
            ..TextDocumentClientCapabilities::default()
        }),
        ..ClientCapabilities::default()
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

    fn write(&self, relative: &str, source: &str) {
        let path = self.directory.path().join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("workspace directory should be created");
        }
        fs::write(path, source).expect("workspace source should be written");
    }

    fn normalize(&self, value: impl serde::Serialize) -> Value {
        let mut value = serde_json::to_value(value).expect("diagnostics should serialize");
        normalize_uri(&mut value, self.root_uri.as_str());
        value
    }
}

fn normalize_uri(value: &mut Value, workspace_uri: &str) {
    match value {
        Value::Array(values) => {
            for value in values {
                normalize_uri(value, workspace_uri);
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                normalize_uri(value, workspace_uri);
            }
        }
        Value::String(value) => {
            *value = value.replace(workspace_uri, "file:///project");
        }
        Value::Null | Value::Bool(_) | Value::Number(_) => {}
    }
}
