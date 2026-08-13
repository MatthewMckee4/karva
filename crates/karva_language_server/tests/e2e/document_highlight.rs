use insta::assert_json_snapshot;
use lsp_types::{
    ClientCapabilities, DidOpenTextDocumentNotification, DidOpenTextDocumentParams,
    DocumentHighlightParams, DocumentHighlightRequest, LanguageKind, PartialResultParams, Position,
    PublishDiagnosticsNotification, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, WorkDoneProgressParams,
};

use super::{TestServer, Workspace};

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
    open(&mut server, custom_uri.clone(), custom_source);
    let custom = server
        .request::<DocumentHighlightRequest>(highlight_params(custom_uri, Position::new(3, 18)));

    let nested_uri = workspace.uri("tests/pkg/test_nested.py");
    open(
        &mut server,
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
    server.receive_notification::<PublishDiagnosticsNotification>();

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

fn highlight_params(uri: Uri, position: Position) -> DocumentHighlightParams {
    DocumentHighlightParams::new(
        WorkDoneProgressParams::default(),
        PartialResultParams::default(),
        TextDocumentPositionParams::new(TextDocumentIdentifier::new(uri), position),
    )
}
