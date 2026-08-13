use insta::assert_json_snapshot;
use lsp_types::{
    ClientCapabilities, DidOpenTextDocumentNotification, DidOpenTextDocumentParams, LanguageKind,
    PartialResultParams, Position, PublishDiagnosticsNotification, ReferenceContext,
    ReferenceParams, ReferencesRequest, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, WorkDoneProgressParams,
};

use super::{TestServer, Workspace};

#[test]
fn finds_disk_and_unsaved_fixture_references() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n\n@fixture\ndef database(): pass\n",
    );
    workspace.write("test_other.py", "def test_other(database): pass\n");
    workspace.write("tests/unrelated.py", "def helper(): pass\n");
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("tests/test_example.py");
    open(
        &mut server,
        uri.clone(),
        "import pytest\n\n@pytest.mark.usefixtures(\"database\")\ndef test_example(database): pass\n",
    );

    let with_declaration = server.request::<ReferencesRequest>(reference_params(
        uri.clone(),
        Position::new(3, 20),
        true,
    ));
    let without_declaration =
        server.request::<ReferencesRequest>(reference_params(uri, Position::new(3, 20), false));

    assert_json_snapshot!(workspace.normalize([with_declaration, without_declaration]), @r#"
    [
      [
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
        },
        {
          "range": {
            "end": {
              "character": 23,
              "line": 0
            },
            "start": {
              "character": 15,
              "line": 0
            }
          },
          "uri": "file:///project/test_other.py"
        },
        {
          "range": {
            "end": {
              "character": 34,
              "line": 2
            },
            "start": {
              "character": 26,
              "line": 2
            }
          },
          "uri": "file:///project/tests/test_example.py"
        },
        {
          "range": {
            "end": {
              "character": 25,
              "line": 3
            },
            "start": {
              "character": 17,
              "line": 3
            }
          },
          "uri": "file:///project/tests/test_example.py"
        }
      ],
      [
        {
          "range": {
            "end": {
              "character": 23,
              "line": 0
            },
            "start": {
              "character": 15,
              "line": 0
            }
          },
          "uri": "file:///project/test_other.py"
        },
        {
          "range": {
            "end": {
              "character": 34,
              "line": 2
            },
            "start": {
              "character": 26,
              "line": 2
            }
          },
          "uri": "file:///project/tests/test_example.py"
        },
        {
          "range": {
            "end": {
              "character": 25,
              "line": 3
            },
            "start": {
              "character": 17,
              "line": 3
            }
          },
          "uri": "file:///project/tests/test_example.py"
        }
      ]
    ]
    "#);
}

#[test]
fn keeps_nested_fixture_references_separate() {
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

    let references =
        server.request::<ReferencesRequest>(reference_params(uri, Position::new(0, 20), true));

    assert_json_snapshot!(workspace.normalize(references), @r#"
    [
      {
        "range": {
          "end": {
            "character": 12,
            "line": 2
          },
          "start": {
            "character": 4,
            "line": 2
          }
        },
        "uri": "file:///project/tests/pkg/conftest.py"
      },
      {
        "range": {
          "end": {
            "character": 24,
            "line": 0
          },
          "start": {
            "character": 16,
            "line": 0
          }
        },
        "uri": "file:///project/tests/pkg/test_nested.py"
      }
    ]
    "#);
}

#[test]
fn uses_custom_public_name_and_utf16_ranges() {
    let workspace = Workspace::new();
    let provider_source =
        "from karva import fixture\n@fixture(name=\"database\")\ndef provider(): pass\n";
    workspace.write("conftest.py", provider_source);
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let provider_uri = workspace.uri("conftest.py");
    open(&mut server, provider_uri.clone(), provider_source);
    let uri = workspace.uri("test_example.py");
    open(&mut server, uri.clone(), "def test_é(database): pass\n");

    let references =
        server.request::<ReferencesRequest>(reference_params(uri, Position::new(0, 15), true));
    let from_provider = server.request::<ReferencesRequest>(reference_params(
        provider_uri,
        Position::new(2, 6),
        true,
    ));
    server.receive_notification::<PublishDiagnosticsNotification>();

    assert_eq!(from_provider, references);

    assert_json_snapshot!(workspace.normalize(references), @r#"
    [
      {
        "range": {
          "end": {
            "character": 23,
            "line": 1
          },
          "start": {
            "character": 15,
            "line": 1
          }
        },
        "uri": "file:///project/conftest.py"
      },
      {
        "range": {
          "end": {
            "character": 19,
            "line": 0
          },
          "start": {
            "character": 11,
            "line": 0
          }
        },
        "uri": "file:///project/test_example.py"
      }
    ]
    "#);
}

#[test]
fn returns_none_for_unsupported_fixture_targets() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    open(
        &mut server,
        uri.clone(),
        "def test_example(tmp_path, missing): pass\n",
    );

    let builtin = server.request::<ReferencesRequest>(reference_params(
        uri.clone(),
        Position::new(0, 20),
        true,
    ));
    let missing =
        server.request::<ReferencesRequest>(reference_params(uri, Position::new(0, 30), true));

    assert_eq!(builtin, None);
    assert_eq!(missing, None);
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

fn reference_params(uri: Uri, position: Position, include_declaration: bool) -> ReferenceParams {
    ReferenceParams::new(
        ReferenceContext::new(include_declaration),
        WorkDoneProgressParams::default(),
        PartialResultParams::default(),
        TextDocumentPositionParams::new(TextDocumentIdentifier::new(uri), position),
    )
}
