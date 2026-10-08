use insta::assert_json_snapshot;
use lsp_types::{
    ClientCapabilities, DidOpenTextDocumentNotification, DidOpenTextDocumentParams, LanguageKind,
    Position, PrepareRenameParams, PrepareRenameRequest, PublishDiagnosticsNotification, Range,
    RenameParams, RenameRequest, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, WorkDoneProgressParams,
};

use super::{TestServer, Workspace};

#[test]
fn prepares_and_renames_fixture_occurrences_across_files() {
    let workspace = Workspace::new();
    let provider_source =
        "from karva import fixture\n@fixture(name=\"data\\x62ase\")\ndef provider(): pass\n";
    workspace.write("conftest.py", provider_source);
    workspace.write("tests/test_other.py", "def test_other(database): pass\n");
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let provider_uri = workspace.uri("conftest.py");
    open(&mut server, provider_uri.clone(), provider_source);
    let uri = workspace.uri("tests/test_example.py");
    open(
        &mut server,
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
    server.receive_notification::<PublishDiagnosticsNotification>();

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
    open(
        &mut server,
        uri.clone(),
        "def test_nested(database): pass\n",
    );

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
    open(
        &mut server,
        uri.clone(),
        "def test_example(database): pass\n",
    );
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
    open(
        &mut server,
        uri.clone(),
        "def test_nested(database): pass\n",
    );

    let edit =
        server.request::<RenameRequest>(rename_params(uri, Position::new(0, 20), "replacement"));

    assert_eq!(edit, None);
}

#[test]
fn renames_default_fixture_through_import_alias_preserving_alias() {
    let workspace = Workspace::new();
    workspace.write(
        "support.py",
        "from karva import fixture\n@fixture\ndef database(): pass\n",
    );
    workspace.write("conftest.py", "from support import database as db\n");
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_query.py");
    open(&mut server, uri.clone(), "def test_query(database): pass\n");

    let edit = server
        .request::<RenameRequest>(rename_params(uri, Position::new(0, 17), "renamed_database"))
        .expect("imported fixture should rename");
    let changes = edit.changes.expect("rename should return text changes");
    assert_eq!(
        changes[&workspace.uri("support.py")][0].new_text,
        "renamed_database"
    );
    assert_eq!(
        changes[&workspace.uri("support.py")][0].range,
        Range::new(Position::new(2, 4), Position::new(2, 12))
    );
    assert_eq!(
        changes[&workspace.uri("conftest.py")][0].new_text,
        "renamed_database"
    );
    assert_eq!(
        changes[&workspace.uri("conftest.py")][0].range,
        Range::new(Position::new(0, 20), Position::new(0, 28))
    );
    assert_eq!(
        changes[&workspace.uri("test_query.py")][0].new_text,
        "renamed_database"
    );
    assert_eq!(
        changes[&workspace.uri("test_query.py")][0].range,
        Range::new(Position::new(0, 15), Position::new(0, 23))
    );
    assert_eq!(changes[&workspace.uri("conftest.py")].len(), 1);
}

#[test]
fn renames_custom_fixture_without_changing_provider_imports() {
    let workspace = Workspace::new();
    workspace.write(
        "support.py",
        "from karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\n",
    );
    workspace.write("conftest.py", "from support import provider as db\n");
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_query.py");
    open(&mut server, uri.clone(), "def test_query(database): pass\n");

    let edit = server
        .request::<RenameRequest>(rename_params(uri, Position::new(0, 17), "renamed_database"))
        .expect("custom fixture should rename");
    let changes = edit.changes.expect("rename should return text changes");
    assert_eq!(
        changes[&workspace.uri("support.py")][0].new_text,
        "renamed_database"
    );
    assert_eq!(
        changes[&workspace.uri("support.py")][0].range,
        Range::new(Position::new(1, 15), Position::new(1, 23))
    );
    assert_eq!(
        changes[&workspace.uri("test_query.py")][0].new_text,
        "renamed_database"
    );
    assert_eq!(
        changes[&workspace.uri("test_query.py")][0].range,
        Range::new(Position::new(0, 15), Position::new(0, 23))
    );
    assert!(!changes.contains_key(&workspace.uri("conftest.py")));
}

#[rstest::rstest]
fn renames_parameter_body_references(
    #[values(
        "    assert database == 'hello'\n",
        "    def nested():\n        return database\n    assert nested()\n",
        "    assert [database for _ in range(1)]\n",
        "    assert (lambda: database)()\n"
    )]
    body: &str,
    #[values("def test_example", "@fixture\ndef wrapper")] declaration: &str,
) {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture\ndef database(): return 'hello'\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    open(
        &mut server,
        workspace.uri("conftest.py"),
        "from karva import fixture\n@fixture\ndef database(): return 'hello'\n",
    );
    let uri = workspace.uri("test_example.py");
    let source = format!("from karva import fixture\n{declaration}(database):\n{body}");
    open(&mut server, uri.clone(), &source);
    let line = if declaration.starts_with('@') { 2 } else { 1 };
    let position = Position::new(line, if line == 2 { 12 } else { 17 });

    let prepared = server.request::<PrepareRenameRequest>(prepare_params(uri.clone(), position));
    let edit = server.request::<RenameRequest>(rename_params(uri, position, "welcome"));
    let provider_edit = server.request::<RenameRequest>(rename_params(
        workspace.uri("conftest.py"),
        Position::new(2, 6),
        "welcome",
    ));

    server.receive_notification::<PublishDiagnosticsNotification>();
    assert!(prepared.is_some());
    let edit = edit.expect("body references should be renamed");
    let provider_edit = provider_edit.expect("provider definition should be renamed");
    assert_eq!(provider_edit, edit);
    let changes = edit.changes.expect("rename should return text changes");
    assert_eq!(
        changes.values().map(Vec::len).sum::<usize>(),
        3,
        "provider, parameter, and body reference should all be edited"
    );
}

fn open(server: &mut TestServer, uri: Uri, source: &str) {
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
