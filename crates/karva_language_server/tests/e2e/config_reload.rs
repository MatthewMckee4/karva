use lsp_types::{
    ClientCapabilities, DefinitionParams, DefinitionRequest,
    DidChangeWatchedFilesClientCapabilities, DidChangeWatchedFilesNotification,
    DidChangeWatchedFilesParams, DidOpenTextDocumentNotification, DidOpenTextDocumentParams,
    FileChangeType, FileEvent, LanguageKind, PartialResultParams, Position,
    PublishDiagnosticsNotification, RegistrationRequest, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, WorkDoneProgressParams, WorkspaceClientCapabilities,
    WorkspaceSymbolParams, WorkspaceSymbolRequest,
};

use super::{TestServer, Workspace};

#[test]
fn registers_and_accepts_source_file_changes() {
    let mut server = TestServer::new(ClientCapabilities {
        workspace: Some(WorkspaceClientCapabilities {
            did_change_watched_files: Some(DidChangeWatchedFilesClientCapabilities {
                dynamic_registration: Some(true),
                relative_pattern_support: Some(false),
            }),
            ..WorkspaceClientCapabilities::default()
        }),
        ..ClientCapabilities::default()
    });
    let (id, params) = server.receive_request::<RegistrationRequest>();
    let registration = params
        .registrations
        .first()
        .expect("file watcher should be registered");
    assert_eq!(registration.method, "workspace/didChangeWatchedFiles");
    let options = registration
        .register_options
        .as_ref()
        .expect("file watcher options should be present");
    assert_eq!(
        options["watchers"].as_array().map(Vec::len),
        Some(6),
        "Python, configuration, and ignore files should be watched"
    );
    server.respond::<RegistrationRequest>(id, ());

    server.notify::<DidChangeWatchedFilesNotification>(DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: Uri::parse("file:///workspace/karva.toml").expect("test URI should be valid"),
            kind: FileChangeType::Changed,
        }],
    });
}

#[rstest::rstest]
fn disk_refresh_is_explicit_for_watched_clients_and_automatic_otherwise(
    #[values(true, false)] watched: bool,
) {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture\ndef database(): pass\n",
    );
    let mut capabilities = ClientCapabilities::default();
    if watched {
        capabilities.workspace = Some(WorkspaceClientCapabilities {
            did_change_watched_files: Some(DidChangeWatchedFilesClientCapabilities {
                dynamic_registration: Some(true),
                ..DidChangeWatchedFilesClientCapabilities::default()
            }),
            ..WorkspaceClientCapabilities::default()
        });
    }
    let mut server = TestServer::with_workspace(capabilities, workspace.folder());
    if watched {
        let (id, _) = server.receive_request::<RegistrationRequest>();
        server.respond::<RegistrationRequest>(id, ());
    }
    let uri = workspace.uri("test_example.py");
    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: LanguageKind::Python,
            version: 1,
            text: "def test_example(database): pass\n".to_owned(),
        },
    });
    server.receive_notification::<PublishDiagnosticsNotification>();
    let params = DefinitionParams::new(
        WorkDoneProgressParams::default(),
        PartialResultParams::default(),
        TextDocumentPositionParams::new(TextDocumentIdentifier::new(uri), Position::new(0, 20)),
    );
    let first = serde_json::to_value(server.request::<DefinitionRequest>(params.clone()))
        .expect("definition JSON");
    assert_eq!(first["range"]["start"]["line"], 2);
    let symbols = WorkspaceSymbolParams::new(
        "database".to_owned(),
        WorkDoneProgressParams::default(),
        PartialResultParams::default(),
    );
    let first_symbols =
        serde_json::to_value(server.request::<WorkspaceSymbolRequest>(symbols.clone()))
            .expect("symbol JSON");
    assert_eq!(first_symbols[0]["location"]["range"]["start"]["line"], 2);
    workspace.write(
        "conftest.py",
        "from karva import fixture\n\n\n@fixture\ndef database(): pass\n",
    );
    let before_notification =
        serde_json::to_value(server.request::<DefinitionRequest>(params.clone()))
            .expect("definition JSON");
    assert_eq!(
        before_notification["range"]["start"]["line"],
        if watched { 2 } else { 4 }
    );
    let before_symbols =
        serde_json::to_value(server.request::<WorkspaceSymbolRequest>(symbols.clone()))
            .expect("symbol JSON");
    assert_eq!(
        before_symbols[0]["location"]["range"]["start"]["line"],
        if watched { 2 } else { 4 }
    );
    if watched {
        server.notify::<DidChangeWatchedFilesNotification>(DidChangeWatchedFilesParams {
            changes: vec![FileEvent {
                uri: workspace.uri("conftest.py"),
                kind: FileChangeType::Changed,
            }],
        });
        server.receive_notification::<PublishDiagnosticsNotification>();
    }
    let refreshed =
        serde_json::to_value(server.request::<DefinitionRequest>(params)).expect("definition JSON");
    assert_eq!(refreshed["range"]["start"]["line"], 4);
    let refreshed_symbols = serde_json::to_value(server.request::<WorkspaceSymbolRequest>(symbols))
        .expect("symbol JSON");
    assert_eq!(
        refreshed_symbols[0]["location"]["range"]["start"]["line"],
        4
    );
}
