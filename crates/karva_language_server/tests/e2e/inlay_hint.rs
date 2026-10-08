use lsp_types::{
    ClientCapabilities, DidChangeTextDocumentNotification, DidChangeTextDocumentParams,
    DidOpenTextDocumentNotification, DidOpenTextDocumentParams, GeneralClientCapabilities,
    InlayHint, InlayHintParams, InlayHintRefreshRequest, InlayHintRequest,
    InlayHintWorkspaceClientCapabilities, LanguageKind, Position, PositionEncodingKind,
    PublishDiagnosticsNotification, Range, TextDocumentContentChangeEvent,
    TextDocumentContentChangeWholeDocument, TextDocumentIdentifier, TextDocumentItem, Uri,
    VersionedTextDocumentIdentifier, WorkDoneProgressParams, WorkspaceClientCapabilities,
};

use super::{TestServer, Workspace};

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

fn params(uri: Uri, range: Range) -> InlayHintParams {
    InlayHintParams::new(
        TextDocumentIdentifier::new(uri),
        range,
        WorkDoneProgressParams::default(),
    )
}

fn hints(server: &mut TestServer, uri: Uri, source: &str) -> Vec<InlayHint> {
    server
        .request::<InlayHintRequest>(params(
            uri,
            Range::new(
                Position::new(0, 0),
                Position::new(source.lines().count().try_into().expect("source fits"), 0),
            ),
        ))
        .expect("hints response should be present")
}

#[test]
fn hints_resolved_injection_sites_without_python_type_annotations() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    assert!(
        server
            .initialization_result()
            .capabilities
            .inlay_hint_provider
            .is_some()
    );
    let uri = workspace.uri("test_hints.py");
    let source = concat!(
        "import pytest\nfrom karva import fixture\n",
        "@fixture(name='database', scope='module')\ndef provider(): pass\n",
        "@fixture\ndef wrapper(database): pass\n",
        "@pytest.mark.usefixtures('database', 'tmp_path', 'unknown')\n",
        "def test_example(database, tmp_path, missing): pass\n",
        "@pytest.mark.parametrize('value', [1])\ndef test_param(value): pass\n",
        "def helper(database): pass\n",
    );
    open(&mut server, uri.clone(), source);
    let result = hints(&mut server, uri, source);
    insta::assert_json_snapshot!(workspace.normalize(result), @r#"
    [
      {
        "kind": 1,
        "label": ": unknown",
        "position": {
          "character": 20,
          "line": 5
        },
        "tooltip": "Karva fixture `database`\nScope: module\nProvider: /project/test_hints.py"
      },
      {
        "kind": 1,
        "label": ": unknown",
        "position": {
          "character": 35,
          "line": 6
        },
        "tooltip": "Karva fixture `database`\nScope: module\nProvider: /project/test_hints.py"
      },
      {
        "kind": 1,
        "label": ": unknown",
        "position": {
          "character": 47,
          "line": 6
        },
        "tooltip": "Karva fixture `tmp_path`\nScope: function\nProvider: Karva built-in"
      },
      {
        "kind": 1,
        "label": ": unknown",
        "position": {
          "character": 25,
          "line": 7
        },
        "tooltip": "Karva fixture `database`\nScope: module\nProvider: /project/test_hints.py"
      },
      {
        "kind": 1,
        "label": ": unknown",
        "position": {
          "character": 35,
          "line": 7
        },
        "tooltip": "Karva fixture `tmp_path`\nScope: function\nProvider: Karva built-in"
      }
    ]
    "#);
}

#[test]
fn range_limits_hints_and_empty_ranges_return_no_hints() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_hints.py");
    let source = "def test_one(tmp_path): pass\ndef test_two(tmp_path): pass\n";
    open(&mut server, uri.clone(), source);
    let range = Range::new(Position::new(1, 0), Position::new(2, 0));
    let result = server
        .request::<InlayHintRequest>(params(uri.clone(), range))
        .expect("hints response");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].position, Position::new(1, 21));
    let boundary = Range::new(Position::new(1, 0), Position::new(1, 21));
    assert!(
        server
            .request::<InlayHintRequest>(params(uri.clone(), boundary))
            .expect("hints response")
            .is_empty()
    );
    let empty = Range::new(Position::new(1, 21), Position::new(1, 21));
    assert!(
        server
            .request::<InlayHintRequest>(params(uri, empty))
            .expect("hints response")
            .is_empty()
    );
}

#[rstest::rstest]
fn uses_negotiated_unicode_positions(
    #[values(PositionEncodingKind::UTF16, PositionEncodingKind::UTF8)]
    encoding: PositionEncodingKind,
) {
    let workspace = Workspace::new();
    let capabilities = ClientCapabilities {
        general: Some(GeneralClientCapabilities {
            position_encodings: Some(vec![encoding.clone()]),
            ..GeneralClientCapabilities::default()
        }),
        ..ClientCapabilities::default()
    };
    let mut server = TestServer::with_workspace(capabilities, workspace.folder());
    let uri = workspace.uri("test_hints.py");
    let prefix = "def test_unicode(value: '😀', tmp_path";
    let source = format!("{prefix}): pass\n");
    open(&mut server, uri.clone(), &source);
    let result = hints(&mut server, uri, &source);
    let character = if encoding == PositionEncodingKind::UTF16 {
        prefix.encode_utf16().count()
    } else {
        prefix.len()
    };
    assert_eq!(result.len(), 1);
    assert_eq!(
        result[0].position,
        Position::new(0, character.try_into().expect("source fits"))
    );
}

#[test]
fn nearest_provider_and_unsaved_overlays_determine_scope() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture(scope='session')\ndef database(): pass\n",
    );
    workspace.write(
        "pkg/conftest.py",
        "from karva import fixture\n@fixture(scope='module')\ndef database(): pass\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("pkg/test_hints.py");
    let source = "def test_example(database): pass\n";
    open(&mut server, uri.clone(), source);
    let before = hints(&mut server, uri.clone(), source);
    assert_eq!(before[0].label, ": unknown".into());
    let provider = workspace.uri("pkg/conftest.py");
    open(
        &mut server,
        provider,
        "from karva import fixture\n@fixture(scope='function')\ndef database(): pass\n",
    );
    let after = hints(&mut server, uri, source);
    server.receive_notification::<PublishDiagnosticsNotification>();
    assert_eq!(after[0].label, ": unknown".into());
    insta::assert_json_snapshot!(workspace.normalize(after), @r#"
    [
      {
        "kind": 1,
        "label": ": unknown",
        "position": {
          "character": 25,
          "line": 0
        },
        "tooltip": "Karva fixture `database`\nScope: function\nProvider: /project/pkg/conftest.py"
      }
    ]
    "#);
}

#[test]
fn cancels_inlay_hints_without_leaving_a_request_pending() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_hints.py");
    let source = "def test_example(tmp_path): pass\n";
    open(&mut server, uri.clone(), source);
    let id = server.send_request::<InlayHintRequest>(params(
        uri.clone(),
        Range::new(Position::new(0, 0), Position::new(1, 0)),
    ));
    server.cancel(&id);
    let error = server
        .receive_response(&id)
        .response_result
        .expect_err("cancelled hint request should fail");
    assert_eq!(error.code, lsp_server::ErrorCode::RequestCanceled as i32);
    assert_eq!(hints(&mut server, uri, source).len(), 1);
}

#[test]
fn rejects_hints_from_stale_document_versions() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_hints.py");
    open(
        &mut server,
        uri.clone(),
        "def test_example(tmp_path): pass\n",
    );
    let id = server.send_request::<InlayHintRequest>(params(
        uri.clone(),
        Range::new(Position::new(0, 0), Position::new(1, 0)),
    ));
    server.notify::<DidChangeTextDocumentNotification>(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            text_document_identifier: TextDocumentIdentifier::new(uri.clone()),
            version: 2,
        },
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument {
                    text: "def test_example(): pass\n".to_owned(),
                },
            ),
        ],
    });
    let error = server
        .receive_response(&id)
        .response_result
        .expect_err("stale hint request should fail");
    server.receive_notification::<PublishDiagnosticsNotification>();
    assert_eq!(error.code, lsp_server::ErrorCode::ContentModified as i32);
    assert!(hints(&mut server, uri, "def test_example(): pass\n").is_empty());
}

#[test]
fn refreshes_consumers_after_unsaved_provider_changes() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture(scope='module')\ndef database(): pass\n",
    );
    let capabilities = ClientCapabilities {
        workspace: Some(WorkspaceClientCapabilities {
            inlay_hint: Some(InlayHintWorkspaceClientCapabilities::new(Some(true))),
            ..WorkspaceClientCapabilities::default()
        }),
        ..ClientCapabilities::default()
    };
    let mut server = TestServer::with_workspace(capabilities, workspace.folder());
    let consumer = workspace.uri("test_hints.py");
    let source = "def test_example(database): pass\n";
    open(&mut server, consumer.clone(), source);
    let (first_id, ()) = server.receive_request::<InlayHintRefreshRequest>();
    // Leave the first refresh pending while the provider is opened and changed.
    let provider = workspace.uri("conftest.py");
    open(
        &mut server,
        provider.clone(),
        "from karva import fixture\n@fixture(scope='module')\ndef database(): pass\n",
    );
    server.receive_notification::<PublishDiagnosticsNotification>();
    let (second_id, ()) = server.receive_request::<InlayHintRefreshRequest>();
    assert_ne!(first_id, second_id);
    server.respond::<InlayHintRefreshRequest>(second_id.clone(), ());
    server.notify::<DidChangeTextDocumentNotification>(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier::new(
            2,
            TextDocumentIdentifier::new(provider),
        ),
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument::new(
                    "from karva import fixture\n@fixture(scope='session')\ndef database(): pass\n"
                        .to_owned(),
                ),
            ),
        ],
    });
    server.receive_notification::<PublishDiagnosticsNotification>();
    server.receive_notification::<PublishDiagnosticsNotification>();
    let (third_id, ()) = server.receive_request::<InlayHintRefreshRequest>();
    assert_ne!(third_id, first_id);
    assert_ne!(third_id, second_id);
    server.respond::<InlayHintRefreshRequest>(third_id, ());
    server.respond::<InlayHintRefreshRequest>(first_id, ());
    assert_eq!(
        hints(&mut server, consumer, source)[0].label,
        ": unknown".into()
    );
}

#[rstest::rstest]
#[case("return True", ": bool")]
#[case("value = len('hello')\n    return value", ": int")]
#[case("yield 'hello'", ": str")]
#[case("return missing()", ": unknown")]
fn hints_inferred_fixture_values(#[case] body: &str, #[case] expected: &str) {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_hints.py");
    let source = format!(
        "from karva import fixture\n@fixture\ndef sample():\n    {body}\ndef test_example(sample): pass\n"
    );
    open(&mut server, uri.clone(), &source);
    let result = hints(&mut server, uri, &source);
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].label, expected.into());
    assert_eq!(result[0].kind, Some(lsp_types::InlayHintKind::Type));
}
