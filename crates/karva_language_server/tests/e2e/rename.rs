use std::fs;

use insta::assert_json_snapshot;
use lsp_types::{
    ClientCapabilities, DidOpenTextDocumentNotification, DidOpenTextDocumentParams, LanguageKind,
    Position, PrepareRenameParams, PrepareRenameRequest, PublishDiagnosticsNotification,
    RenameParams, RenameRequest, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, WorkDoneProgressParams, WorkspaceFolder,
};
use serde_json::Value;
use tempfile::TempDir;

use super::TestServer;

#[test]
fn prepares_and_renames_fixture_occurrences_across_files() {
    let workspace = Workspace::new();
    let provider_source =
        "from karva import fixture\n@fixture(name=\"data\\x62ase\")\ndef provider(): pass\n";
    workspace.write("conftest.py", provider_source);
    workspace.write("tests/test_other.py", "def test_other(database): pass\n");
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let provider_uri = workspace.uri("conftest.py");
    open(&server, provider_uri.clone(), provider_source);
    let uri = workspace.uri("tests/test_example.py");
    open(
        &server,
        uri.clone(),
        "import pytest\n\n@pytest.mark.usefixtures(\"data\\x62ase\")\ndef test_example(database): pass\n",
    );

    let position = Position::new(3, 20);
    let prepared = server.request::<PrepareRenameRequest>(prepare_params(uri.clone(), position));
    let edit = server.request::<RenameRequest>(rename_params(uri, position, "данные"));
    let provider_prepared = server
        .request::<PrepareRenameRequest>(prepare_params(provider_uri.clone(), Position::new(2, 6)));
    let provider_edit =
        server.request::<RenameRequest>(rename_params(provider_uri, Position::new(2, 6), "данные"));

    assert_eq!(provider_edit, edit);
    assert_json_snapshot!(workspace.normalize((prepared, provider_prepared, edit)));
}

#[test]
fn renames_only_the_selected_nested_provider() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture\ndef database(): pass\n",
    );
    workspace.write("tests/test_root.py", "def test_root(database): pass\n");
    workspace.write(
        "tests/pkg/conftest.py",
        "from karva import fixture\n@fixture\ndef database(): pass\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("tests/pkg/test_nested.py");
    open(&server, uri.clone(), "def test_nested(database): pass\n");

    let edit = server.request::<RenameRequest>(rename_params(
        uri,
        Position::new(0, 20),
        "nested_database",
    ));

    assert_json_snapshot!(workspace.normalize(edit));
}

#[test]
fn rejects_partial_and_invalid_fixture_renames() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture(name='data' 'base')\ndef provider(): pass\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    open(&server, uri.clone(), "def test_example(database): pass\n");
    let position = Position::new(0, 20);

    let prepared = server.request::<PrepareRenameRequest>(prepare_params(uri.clone(), position));
    let partial =
        server.request::<RenameRequest>(rename_params(uri.clone(), position, "renamed_database"));
    let invalid_id = server.send_request::<RenameRequest>(rename_params(uri, position, "class"));
    let invalid = server.receive_response(&invalid_id);

    assert_eq!(prepared, None);
    assert_eq!(partial, None);
    let error = invalid
        .response_result
        .expect_err("invalid identifier should fail");
    assert_eq!(error.code, lsp_server::ErrorCode::InvalidParams as i32);
    assert_eq!(
        error.message,
        "fixture rename `class` is not a valid Python identifier"
    );
}

#[test]
fn rejects_name_that_would_select_a_nested_provider() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture\ndef database(): pass\n",
    );
    workspace.write(
        "tests/pkg/conftest.py",
        "from karva import fixture\n@fixture\ndef replacement(): pass\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("tests/pkg/test_nested.py");
    open(&server, uri.clone(), "def test_nested(database): pass\n");

    let edit =
        server.request::<RenameRequest>(rename_params(uri, Position::new(0, 20), "replacement"));

    assert_eq!(edit, None);
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

fn prepare_params(uri: Uri, position: Position) -> PrepareRenameParams {
    PrepareRenameParams::new(
        WorkDoneProgressParams::default(),
        TextDocumentPositionParams::new(TextDocumentIdentifier::new(uri), position),
    )
}

fn rename_params(uri: Uri, position: Position, new_name: &str) -> RenameParams {
    RenameParams::new(
        new_name.to_owned(),
        WorkDoneProgressParams::default(),
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
        let mut value = serde_json::to_value(value).expect("rename should serialize");
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
            let original = std::mem::take(values);
            for (key, mut value) in original {
                normalize_paths(&mut value, workspace_uri, workspace_path);
                values.insert(
                    key.replace(workspace_uri, "file:///project")
                        .replace(workspace_path, "/project"),
                    value,
                );
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
