use std::fs;

use insta::assert_json_snapshot;
use lsp_types::{
    ClientCapabilities, CompletionList, CompletionParams, CompletionRequest, CompletionResponse,
    DidChangeTextDocumentNotification, DidChangeTextDocumentParams,
    DidOpenTextDocumentNotification, DidOpenTextDocumentParams, LanguageKind, PartialResultParams,
    Position, PublishDiagnosticsNotification, TextDocumentContentChangeEvent,
    TextDocumentContentChangeWholeDocument, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, VersionedTextDocumentIdentifier, WorkDoneProgressParams,
    WorkspaceFolder,
};
use serde_json::Value;
use tempfile::TempDir;

use super::TestServer;

#[test]
fn completes_disk_and_builtin_fixtures_in_unsaved_test_parameters() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n\n@fixture(scope=\"session\")\ndef tmp_data(): pass\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    let source = "def test_example(tmp): pass\n";

    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: LanguageKind::Python,
            version: 1,
            text: source.to_owned(),
        },
    });
    server.receive_notification::<PublishDiagnosticsNotification>();

    let response =
        server.request::<CompletionRequest>(completion_params(uri, Position::new(0, 20)));

    assert_json_snapshot!(workspace.normalize(response), @r###"
    {
      "isIncomplete": false,
      "items": [
        {
          "detail": "fixture · /project/conftest.py · scope=session · autouse=false",
          "kind": 6,
          "label": "tmp_data",
          "textEdit": {
            "newText": "tmp_data",
            "range": {
              "end": {
                "character": 20,
                "line": 0
              },
              "start": {
                "character": 17,
                "line": 0
              }
            }
          }
        },
        {
          "detail": "fixture · Karva built-in · scope=function · autouse=false",
          "kind": 6,
          "label": "tmp_path",
          "textEdit": {
            "newText": "tmp_path",
            "range": {
              "end": {
                "character": 20,
                "line": 0
              },
              "start": {
                "character": 17,
                "line": 0
              }
            }
          }
        },
        {
          "detail": "fixture · Karva built-in · scope=session · autouse=false",
          "kind": 6,
          "label": "tmp_path_factory",
          "textEdit": {
            "newText": "tmp_path_factory",
            "range": {
              "end": {
                "character": 20,
                "line": 0
              },
              "start": {
                "character": 17,
                "line": 0
              }
            }
          }
        },
        {
          "detail": "fixture · Karva built-in · scope=function · autouse=false",
          "kind": 6,
          "label": "tmpdir",
          "textEdit": {
            "newText": "tmpdir",
            "range": {
              "end": {
                "character": 20,
                "line": 0
              },
              "start": {
                "character": 17,
                "line": 0
              }
            }
          }
        },
        {
          "detail": "fixture · Karva built-in · scope=session · autouse=false",
          "kind": 6,
          "label": "tmpdir_factory",
          "textEdit": {
            "newText": "tmpdir_factory",
            "range": {
              "end": {
                "character": 20,
                "line": 0
              },
              "start": {
                "character": 17,
                "line": 0
              }
            }
          }
        }
      ]
    }
    "###);
}

#[test]
fn completes_use_fixtures_strings() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n\n@fixture\ndef database(): pass\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    let source = "import karva\n\n@karva.tags.use_fixtures(\"dat\")\ndef test_example(): pass\n";

    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: LanguageKind::Python,
            version: 1,
            text: source.to_owned(),
        },
    });
    server.receive_notification::<PublishDiagnosticsNotification>();

    let response =
        server.request::<CompletionRequest>(completion_params(uri, Position::new(2, 28)));

    assert_json_snapshot!(workspace.normalize(response), @r###"
    {
      "isIncomplete": false,
      "items": [
        {
          "detail": "fixture · /project/conftest.py · scope=function · autouse=false",
          "kind": 6,
          "label": "database",
          "textEdit": {
            "newText": "database",
            "range": {
              "end": {
                "character": 29,
                "line": 2
              },
              "start": {
                "character": 26,
                "line": 2
              }
            }
          }
        }
      ]
    }
    "###);
}

#[test]
fn returns_no_completion_outside_fixture_context() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("helpers.py");
    let source = "def helper(dat): pass\n";

    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: LanguageKind::Python,
            version: 1,
            text: source.to_owned(),
        },
    });
    server.receive_notification::<PublishDiagnosticsNotification>();

    let response =
        server.request::<CompletionRequest>(completion_params(uri, Position::new(0, 14)));

    assert_eq!(response, None);
}

#[test]
fn refreshes_disk_sources_without_dynamic_file_watching() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture\ndef provider_before(): pass\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    open(&server, uri.clone(), "def test_example(provider_): pass\n");

    let first =
        server.request::<CompletionRequest>(completion_params(uri.clone(), Position::new(0, 26)));
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture\ndef provider_after(): pass\n",
    );
    let second = server.request::<CompletionRequest>(completion_params(uri, Position::new(0, 26)));

    assert!(matches!(
        first,
        Some(CompletionResponse::CompletionList(CompletionList { items, .. }))
            if items.iter().any(|item| item.label == "provider_before")
    ));
    assert!(matches!(
        second,
        Some(CompletionResponse::CompletionList(CompletionList { items, .. }))
            if items.iter().any(|item| item.label == "provider_after")
                && items.iter().all(|item| item.label != "provider_before")
    ));
}

#[test]
fn cancels_a_background_completion_once() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    open(&server, uri.clone(), "def test_example(tmp): pass\n");

    let request_id = server
        .send_request::<CompletionRequest>(completion_params(uri.clone(), Position::new(0, 20)));
    server.cancel(&request_id);
    let response = server.receive_response(&request_id);

    let error = response
        .response_result
        .expect_err("cancelled completion should fail");
    assert_eq!(error.code, lsp_server::ErrorCode::RequestCanceled as i32);

    let completion =
        server.request::<CompletionRequest>(completion_params(uri, Position::new(0, 20)));
    assert!(completion.is_some());
}

#[test]
fn rejects_completion_from_a_stale_document_version() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    open(&server, uri.clone(), "def test_example(tmp): pass\n");

    let request_id = server
        .send_request::<CompletionRequest>(completion_params(uri.clone(), Position::new(0, 20)));
    server.notify::<DidChangeTextDocumentNotification>(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            text_document_identifier: TextDocumentIdentifier::new(uri.clone()),
            version: 2,
        },
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument {
                    text: "def test_example(cap): pass\n".to_owned(),
                },
            ),
        ],
    });
    let response = server.receive_response(&request_id);

    let error = response
        .response_result
        .expect_err("stale completion should fail");
    assert_eq!(error.code, lsp_server::ErrorCode::ContentModified as i32);

    let completion =
        server.request::<CompletionRequest>(completion_params(uri, Position::new(0, 20)));
    assert!(completion.is_some());
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

fn completion_params(uri: Uri, position: Position) -> CompletionParams {
    CompletionParams::new(
        None,
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
        let mut value = serde_json::to_value(value).expect("completion should serialize");
        let workspace_path = self.directory.path().to_string_lossy();
        normalize_paths(&mut value, self.root_uri.as_str(), &workspace_path);
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
