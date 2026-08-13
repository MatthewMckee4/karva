use lsp_types::{
    ClientCapabilities, DidChangeWatchedFilesClientCapabilities, DidChangeWatchedFilesNotification,
    DidChangeWatchedFilesParams, FileChangeType, FileEvent, RegistrationRequest, Uri,
    WorkspaceClientCapabilities,
};

use super::TestServer;

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
