use lsp_types::{
    ClientCapabilities, CodeActionContext, CodeActionKind, CodeActionParams, CodeActionRequest,
    DidChangeTextDocumentNotification, DidChangeTextDocumentParams,
    DidOpenTextDocumentNotification, DidOpenTextDocumentParams, LanguageKind, PartialResultParams,
    PublishDiagnosticsNotification, TextDocumentContentChangeEvent,
    TextDocumentContentChangeWholeDocument, TextDocumentIdentifier, TextDocumentItem,
    VersionedTextDocumentIdentifier, WorkDoneProgressParams,
};

use super::{TestServer, Workspace};

fn capabilities() -> ClientCapabilities {
    serde_json::from_value(serde_json::json!({
        "workspace": {"workspaceEdit": {"documentChanges": true}},
        "textDocument": {"codeAction": {
            "codeActionLiteralSupport": {"codeActionKind": {"valueSet": ["quickfix"]}}
        }}
    }))
    .expect("client capabilities")
}

#[rstest::rstest]
fn applies_versioned_local_stub_and_clears_diagnostic(#[values("\n", "\r\n")] newline: &str) {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(capabilities(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    let source = format!("def test_example(данные):{newline}    pass{newline}");
    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: LanguageKind::Python,
            version: 1,
            text: source.clone(),
        },
    });
    let diagnostics = server.receive_notification::<PublishDiagnosticsNotification>();
    let diagnostic = diagnostics
        .diagnostics
        .first()
        .expect("missing fixture diagnostic")
        .clone();
    let params = CodeActionParams::new(
        TextDocumentIdentifier::new(uri.clone()),
        diagnostic.range,
        CodeActionContext::new(vec![diagnostic], None, None),
        WorkDoneProgressParams::default(),
        PartialResultParams::default(),
    );
    let response = server.request::<CodeActionRequest>(params.clone());
    let value = serde_json::to_value(response).expect("actions serialize");
    assert_eq!(value.as_array().expect("action array").len(), 1);
    assert_eq!(
        value[0]["edit"]["documentChanges"][0]["textDocument"]["version"],
        1
    );
    let text = value[0]["edit"]["documentChanges"][0]["edits"][0]["newText"]
        .as_str()
        .expect("inserted stub");
    assert!(text.contains("raise NotImplementedError"));
    let import = value[0]["edit"]["documentChanges"][0]["edits"][1]["newText"]
        .as_str()
        .expect("decorator import");
    let edited = format!("{import}{source}{text}");
    server.notify::<DidChangeTextDocumentNotification>(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            text_document_identifier: TextDocumentIdentifier::new(uri),
            version: 2,
        },
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument { text: edited },
            ),
        ],
    });
    let updated = server.receive_notification::<PublishDiagnosticsNotification>();
    assert!(updated.diagnostics.is_empty());
    let stale = server.request::<CodeActionRequest>(params);
    assert!(stale.expect("action array").is_empty());
    let normalized = workspace.normalize(value);
    let mut normalized = serde_json::to_string(&normalized).expect("serialize snapshot");
    normalized = normalized.replace("\\r\\n", "\\n");
    let normalized: serde_json::Value = serde_json::from_str(&normalized).expect("normalized JSON");
    insta::assert_json_snapshot!(normalized, @r#"
    [
      {
        "diagnostics": [
          {
            "code": "missing-fixture",
            "message": "Test `test_example` requires missing fixture `данные`",
            "range": {
              "end": {
                "character": 23,
                "line": 0
              },
              "start": {
                "character": 17,
                "line": 0
              }
            },
            "severity": 1,
            "source": "karva"
          }
        ],
        "edit": {
          "documentChanges": [
            {
              "edits": [
                {
                  "newText": "\n@fixture\ndef данные():\n    raise NotImplementedError\n",
                  "range": {
                    "end": {
                      "character": 0,
                      "line": 2
                    },
                    "start": {
                      "character": 0,
                      "line": 2
                    }
                  }
                },
                {
                  "newText": "from karva import fixture\n",
                  "range": {
                    "end": {
                      "character": 0,
                      "line": 0
                    },
                    "start": {
                      "character": 0,
                      "line": 0
                    }
                  }
                }
              ],
              "textDocument": {
                "uri": "file:///project/test_example.py",
                "version": 1
              }
            }
          ]
        },
        "kind": "quickfix",
        "title": "Create local fixture `данные`"
      }
    ]
    "#);
}

#[rstest::rstest]
fn respects_action_filters_and_client_edit_capabilities(
    #[values(true, false)] versioned: bool,
    #[values(true, false)] literals: bool,
) {
    let workspace = Workspace::new();
    let mut caps = capabilities();
    if !versioned {
        caps.workspace = None;
    }
    if !literals {
        caps.text_document = None;
    }
    let mut server = TestServer::with_workspace(caps, workspace.folder());
    let uri = workspace.uri("test_example.py");
    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: LanguageKind::Python,
            version: 1,
            text: "def test_example(database): pass\n".to_owned(),
        },
    });
    let diagnostics = server.receive_notification::<PublishDiagnosticsNotification>();
    let diagnostic = diagnostics
        .diagnostics
        .first()
        .expect("missing fixture")
        .clone();
    let params = CodeActionParams::new(
        TextDocumentIdentifier::new(uri),
        diagnostic.range,
        CodeActionContext::new(vec![diagnostic], Some(vec![CodeActionKind::Refactor]), None),
        WorkDoneProgressParams::default(),
        PartialResultParams::default(),
    );
    assert!(
        server
            .request::<CodeActionRequest>(params.clone())
            .expect("actions")
            .is_empty()
    );
    let mut params = params;
    params.context.only = None;
    assert_eq!(
        server
            .request::<CodeActionRequest>(params)
            .expect("actions")
            .len(),
        usize::from(versioned && literals)
    );
}

#[rstest::rstest]
fn ambiguous_or_invalid_fixes_are_suppressed(
    #[values(
        "database = 42\ndef test_example(database): pass\n",
        "fixture = 1\nkarva = 2\ndef test_example(database): pass\n",
        "NotImplementedError = 1\ndef test_example(database): pass\n",
        "import pytest\n@pytest.mark.usefixtures('bad-name')\ndef test_example(): pass\n",
        "from karva import fixture\n@fixture(scope=dynamic)\ndef provider(database): pass\n",
        "from karva import fixture\n@fixture(scope='bad')\ndef database(): pass\ndef test_example(database): pass\n"
    )]
    source: &str,
) {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(capabilities(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: LanguageKind::Python,
            version: 1,
            text: source.to_owned(),
        },
    });
    let diagnostics = server.receive_notification::<PublishDiagnosticsNotification>();
    let range = lsp_types::Range::new(
        lsp_types::Position::new(0, 0),
        lsp_types::Position::new(10, 0),
    );
    let params = CodeActionParams::new(
        TextDocumentIdentifier::new(uri),
        range,
        CodeActionContext::new(diagnostics.diagnostics, None, None),
        WorkDoneProgressParams::default(),
        PartialResultParams::default(),
    );
    assert!(
        server
            .request::<CodeActionRequest>(params)
            .expect("actions")
            .is_empty()
    );
}
