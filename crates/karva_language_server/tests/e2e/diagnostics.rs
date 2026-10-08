use lsp_types::{
    ClientCapabilities, DiagnosticsCapabilities, DidChangeTextDocumentNotification,
    DidChangeTextDocumentParams, DidChangeWatchedFilesNotification, DidChangeWatchedFilesParams,
    DidCloseTextDocumentNotification, DidCloseTextDocumentParams, DidOpenTextDocumentNotification,
    DidOpenTextDocumentParams, FileChangeType, FileEvent, LanguageKind,
    PublishDiagnosticsClientCapabilities, PublishDiagnosticsNotification,
    TextDocumentClientCapabilities, TextDocumentContentChangeEvent,
    TextDocumentContentChangeWholeDocument, TextDocumentIdentifier, TextDocumentItem,
    VersionedTextDocumentIdentifier,
};

use super::{TestServer, Workspace};

#[test]
fn publishes_updates_and_clears_unsaved_diagnostics() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(capabilities(), workspace.folder());
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
    let mut server = TestServer::with_workspace(capabilities(), workspace.folder());
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
    let mut server = TestServer::with_workspace(capabilities(), workspace.folder());
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
    let mut server = TestServer::with_workspace(capabilities(), workspace.folder());
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

#[rstest::rstest]
fn reports_configuration_parse_cause_and_recovers(
    #[values("karva.toml", "pyproject.toml")] config: &str,
) {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(capabilities(), workspace.folder());
    let section = if config == "karva.toml" {
        "profile.default.test"
    } else {
        "tool.karva.profile.default.test"
    };
    workspace.write(
        config,
        &format!("[{section}]\ntest-function-prefix = 123\n"),
    );
    let uri = workspace.uri("test_example.py");
    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: LanguageKind::Python,
            version: 1,
            text: "def test_example(): pass\n".to_owned(),
        },
    });
    let diagnostics = server.receive_notification::<PublishDiagnosticsNotification>();
    let message = &diagnostics
        .diagnostics
        .first()
        .expect("config error should be published")
        .message;
    let message = serde_json::to_value(message).expect("message should serialize");
    let message = message
        .as_str()
        .expect("diagnostic message should be plain text");
    assert!(message.contains(config), "{message}");
    assert!(message.contains("test-function-prefix = 123"), "{message}");
    assert!(message.contains("expected a string"), "{message}");

    workspace.write(
        config,
        &format!("[{section}]\ntest-function-prefix = 'test'\n"),
    );
    server.notify::<DidChangeWatchedFilesNotification>(DidChangeWatchedFilesParams {
        changes: vec![FileEvent {
            uri: workspace.uri(config),
            kind: FileChangeType::Changed,
        }],
    });
    let recovered = server.receive_notification::<PublishDiagnosticsNotification>();
    assert_eq!(recovered.uri, uri);
    assert!(recovered.diagnostics.is_empty());
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

#[rstest::rstest]
fn literal_configuration_values_preserve_missing_fixture_diagnostics(
    #[values(("TIMEOUT = 30\n", true), ("setting = provider\n", false))] case: (&str, bool),
) {
    let workspace = Workspace::new();
    workspace.write("conftest.py", case.0);
    let mut server = TestServer::with_workspace(capabilities(), workspace.folder());
    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: workspace.uri("test_example.py"),
            language_id: LanguageKind::Python,
            version: 1,
            text: "def test_example(missing): pass\n".to_owned(),
        },
    });
    let diagnostics = server.receive_notification::<PublishDiagnosticsNotification>();
    if case.1 {
        insta::assert_json_snapshot!(workspace.normalize(diagnostics), @r#"
        {
          "diagnostics": [
            {
              "code": "missing-fixture",
              "message": "Test `test_example` requires missing fixture `missing`",
              "range": {
                "end": {
                  "character": 24,
                  "line": 0
                },
                "start": {
                  "character": 17,
                  "line": 0
                }
              },
              "relatedInformation": [],
              "severity": 1,
              "source": "karva"
            }
          ],
          "uri": "file:///project/test_example.py",
          "version": 1
        }
        "#);
    } else {
        assert!(diagnostics.diagnostics.is_empty());
    }
}
