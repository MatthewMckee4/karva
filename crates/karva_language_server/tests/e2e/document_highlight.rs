use std::fs;

use insta::assert_json_snapshot;
use lsp_types::{
    ClientCapabilities, DidOpenTextDocumentNotification, DidOpenTextDocumentParams,
    DocumentHighlightParams, DocumentHighlightRequest, LanguageKind, PartialResultParams, Position,
    PublishDiagnosticsNotification, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, WorkDoneProgressParams, WorkspaceFolder,
};
use serde_json::Value;
use tempfile::TempDir;

use super::TestServer;

#[test]
fn highlights_fixture_declarations_reads_nested_providers_and_unsupported_targets() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture\ndef database(): pass\n",
    );
    workspace.write(
        "tests/pkg/conftest.py",
        "from karva import fixture\n@fixture\ndef database(): pass\n",
    );
    workspace.write(
        "tests/pkg/test_nested.py",
        "def test_nested(tmp_path, database, missing): pass\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());

    let custom_uri = workspace.uri("custom.py");
    let custom_source = "from karva import fixture\n@fixture(name=\"данные\")\ndef provider(): pass\ndef test_example(данные): pass\n";
    open(&server, custom_uri.clone(), custom_source);
    let custom = server
        .request::<DocumentHighlightRequest>(highlight_params(custom_uri, Position::new(3, 18)));

    let nested_uri = workspace.uri("tests/pkg/test_nested.py");
    open(
        &server,
        nested_uri.clone(),
        "def test_nested(database): pass\n",
    );
    let nested = server.request::<DocumentHighlightRequest>(highlight_params(
        nested_uri.clone(),
        Position::new(0, 27),
    ));
    let unsupported = server.request::<DocumentHighlightRequest>(highlight_params(
        nested_uri.clone(),
        Position::new(0, 17),
    ));
    let missing = server
        .request::<DocumentHighlightRequest>(highlight_params(nested_uri, Position::new(0, 37)));

    assert_json_snapshot!(workspace.normalize([custom, nested, unsupported, missing]), @r#"
    [
      [
        {
          "kind": 3,
          "range": {
            "end": {
              "character": 21,
              "line": 1
            },
            "start": {
              "character": 15,
              "line": 1
            }
          }
        },
        {
          "kind": 3,
          "range": {
            "end": {
              "character": 12,
              "line": 2
            },
            "start": {
              "character": 4,
              "line": 2
            }
          }
        },
        {
          "kind": 2,
          "range": {
            "end": {
              "character": 23,
              "line": 3
            },
            "start": {
              "character": 17,
              "line": 3
            }
          }
        }
      ],
      null,
      [
        {
          "kind": 2,
          "range": {
            "end": {
              "character": 24,
              "line": 0
            },
            "start": {
              "character": 16,
              "line": 0
            }
          }
        }
      ],
      null
    ]
    "#);
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

fn highlight_params(uri: Uri, position: Position) -> DocumentHighlightParams {
    DocumentHighlightParams::new(
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
        let path = self.directory.path().join(relative);
        fs::create_dir_all(
            path.parent()
                .expect("workspace source should have a parent"),
        )
        .expect("workspace source parent should be created");
        fs::write(path, source).expect("workspace source should be written");
    }

    fn normalize(&self, value: impl serde::Serialize) -> Value {
        let mut value = serde_json::to_value(value).expect("highlights should serialize");
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
