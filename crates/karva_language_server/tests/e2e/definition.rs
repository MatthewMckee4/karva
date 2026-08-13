use std::fs;

use insta::assert_json_snapshot;
use lsp_types::{
    ClientCapabilities, DefinitionParams, DefinitionRequest, DidOpenTextDocumentNotification,
    DidOpenTextDocumentParams, LanguageKind, PartialResultParams, Position,
    PublishDiagnosticsNotification, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, WorkDoneProgressParams, WorkspaceFolder,
};
use serde_json::Value;
use tempfile::TempDir;

use super::TestServer;

#[test]
fn goes_to_fixture_in_disk_ancestor() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n\n@fixture\ndef database(): pass\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    open(&server, uri.clone(), "def test_example(database): pass\n");

    let response =
        server.request::<DefinitionRequest>(definition_params(uri, Position::new(0, 20)));

    assert_json_snapshot!(workspace.normalize(response), @r#"
    {
      "range": {
        "end": {
          "character": 12,
          "line": 3
        },
        "start": {
          "character": 4,
          "line": 3
        }
      },
      "uri": "file:///project/conftest.py"
    }
    "#);
}

#[test]
fn goes_to_unsaved_local_fixture_override() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    let source = concat!(
        "from karva import fixture\n\n",
        "@fixture\n",
        "def local(): pass\n\n",
        "def test_example(local): pass\n",
    );
    open(&server, uri.clone(), source);

    let response =
        server.request::<DefinitionRequest>(definition_params(uri, Position::new(5, 20)));

    assert_json_snapshot!(workspace.normalize(response), @r#"
    {
      "range": {
        "end": {
          "character": 9,
          "line": 3
        },
        "start": {
          "character": 4,
          "line": 3
        }
      },
      "uri": "file:///project/test_example.py"
    }
    "#);
}

#[test]
fn goes_to_unsaved_nested_ancestor_override() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n\n@fixture\ndef root_only(): pass\n",
    );
    workspace.write(
        "package/conftest.py",
        "from karva import fixture\n\n@fixture\ndef database(): pass\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let ancestor_uri = workspace.uri("package/conftest.py");
    open(
        &server,
        ancestor_uri,
        concat!(
            "from karva import fixture\n\n",
            "@fixture\n",
            "def database():\n",
            "    return \"unsaved\"\n",
        ),
    );
    let test_uri = workspace.uri("package/test_example.py");
    open(
        &server,
        test_uri.clone(),
        "def test_example(database): pass\n",
    );

    let response =
        server.request::<DefinitionRequest>(definition_params(test_uri, Position::new(0, 20)));

    assert_json_snapshot!(workspace.normalize(response), @r#"
    {
      "range": {
        "end": {
          "character": 12,
          "line": 3
        },
        "start": {
          "character": 4,
          "line": 3
        }
      },
      "uri": "file:///project/package/conftest.py"
    }
    "#);
}

#[test]
fn resolves_nested_package_fixture_shadowing() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n\n@fixture\ndef root_only(): pass\n",
    );
    workspace.write(
        "package/conftest.py",
        "from karva import fixture\n\n@fixture\ndef nested_only(): pass\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("package/test_example.py");
    open(
        &server,
        uri.clone(),
        "def test_example(nested_only): pass\n",
    );

    let response =
        server.request::<DefinitionRequest>(definition_params(uri, Position::new(0, 20)));

    assert_json_snapshot!(workspace.normalize(response), @r#"
    {
      "range": {
        "end": {
          "character": 15,
          "line": 3
        },
        "start": {
          "character": 4,
          "line": 3
        }
      },
      "uri": "file:///project/package/conftest.py"
    }
    "#);
}

#[test]
fn goes_to_fixture_dependencies_and_usefixtures_strings() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        concat!(
            "from karva import fixture\n\n",
            "@fixture\n",
            "def parent(): pass\n\n",
            "@fixture\n",
            "def child(parent): pass\n",
        ),
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    let source = concat!(
        "import pytest\n\n",
        "@pytest.mark.usefixtures(\"parent\")\n",
        "def test_example(child): pass\n",
    );
    open(&server, uri.clone(), source);

    let child =
        server.request::<DefinitionRequest>(definition_params(uri.clone(), Position::new(3, 20)));
    let parent = server.request::<DefinitionRequest>(definition_params(uri, Position::new(2, 29)));

    assert_json_snapshot!(workspace.normalize([child, parent]), @r#"
    [
      {
        "range": {
          "end": {
            "character": 9,
            "line": 6
          },
          "start": {
            "character": 4,
            "line": 6
          }
        },
        "uri": "file:///project/conftest.py"
      },
      {
        "range": {
          "end": {
            "character": 10,
            "line": 3
          },
          "start": {
            "character": 4,
            "line": 3
          }
        },
        "uri": "file:///project/conftest.py"
      }
    ]
    "#);
}

#[test]
fn returns_no_definition_for_builtin_or_unknown_fixture() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let builtin_uri = workspace.uri("builtin.py");
    open(
        &server,
        builtin_uri.clone(),
        "def test_example(tmp_path): pass\n",
    );
    let builtin =
        server.request::<DefinitionRequest>(definition_params(builtin_uri, Position::new(0, 20)));

    let unknown_uri = workspace.uri("unknown.py");
    open(
        &server,
        unknown_uri.clone(),
        "def test_example(unknown): pass\n",
    );
    let unknown =
        server.request::<DefinitionRequest>(definition_params(unknown_uri, Position::new(0, 20)));

    assert_eq!(builtin, None);
    assert_eq!(unknown, None);
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

fn definition_params(uri: Uri, position: Position) -> DefinitionParams {
    DefinitionParams::new(
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
        fs::create_dir_all(
            self.directory
                .path()
                .join(relative)
                .parent()
                .expect("workspace source should have a parent"),
        )
        .expect("workspace source parent should be created");
        fs::write(self.directory.path().join(relative), source)
            .expect("workspace source should be written");
    }

    fn normalize(&self, value: impl serde::Serialize) -> Value {
        let mut value = serde_json::to_value(value).expect("definition should serialize");
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
