use insta::assert_json_snapshot;
use lsp_types::{
    ClientCapabilities, DidOpenTextDocumentNotification, DidOpenTextDocumentParams,
    HoverClientCapabilities, HoverParams, HoverRequest, LanguageKind, MarkupKind, Position,
    PublishDiagnosticsNotification, TextDocumentClientCapabilities, TextDocumentIdentifier,
    TextDocumentItem, TextDocumentPositionParams, Uri, WorkDoneProgressParams,
};

use super::{TestServer, Workspace};

#[test]
fn hovers_unsaved_fixture_with_metadata_and_dependencies() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n\n@fixture\ndef parent(): pass\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    let source = concat!(
        "from karva import fixture\n\n",
        "@fixture(name=\"database\", scope=\"module\", autouse=True)\n",
        "def local_database(parent):\n",
        "    \"\"\"Local database fixture.\"\"\"\n",
        "    pass\n\n",
        "def test_example(database): pass\n",
    );
    open(&mut server, uri.clone(), source);

    let response = server.request::<HoverRequest>(hover_params(uri, Position::new(7, 19)));

    assert_json_snapshot!(workspace.normalize(response), @r###"
    {
      "contents": {
        "kind": "plaintext",
        "value": "def local_database(parent):\n\nKarva fixture: database\nScope: module\nAutouse: true\nProvider: /project/test_example.py\nDependencies: parent\n\nLocal database fixture."
      },
      "range": {
        "end": {
          "character": 25,
          "line": 7
        },
        "start": {
          "character": 17,
          "line": 7
        }
      }
    }
    "###);
}

#[test]
fn hovers_builtin_fixture_with_markdown_preference() {
    let workspace = Workspace::new();
    let capabilities = ClientCapabilities {
        text_document: Some(TextDocumentClientCapabilities {
            hover: Some(HoverClientCapabilities {
                content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                ..HoverClientCapabilities::default()
            }),
            ..TextDocumentClientCapabilities::default()
        }),
        ..ClientCapabilities::default()
    };
    let mut server = TestServer::with_workspace(capabilities, workspace.folder());
    let uri = workspace.uri("test_example.py");
    open(
        &mut server,
        uri.clone(),
        "def test_example(tmp_path): pass\n",
    );

    let response = server.request::<HoverRequest>(hover_params(uri, Position::new(0, 20)));

    assert_json_snapshot!(workspace.normalize(response), @r#"
    {
      "contents": {
        "kind": "markdown",
        "value": "```python\ntmp_path(tmp_path_factory: TempPathFactory) -> Path\n```\n\nKarva fixture **tmp\\_path**\n\nScope: `function`  \nAutouse: `false`  \nProvider: Karva built-in\n\nProvide a temporary directory as a \\`pathlib.Path\\` object."
      },
      "range": {
        "end": {
          "character": 25,
          "line": 0
        },
        "start": {
          "character": 17,
          "line": 0
        }
      }
    }
    "#);
}

#[test]
fn hovers_pytest_usefixtures_string() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n\n@fixture\ndef database(): pass\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    let source = concat!(
        "import pytest\n\n",
        "@pytest.mark.usefixtures(\"database\")\n",
        "def test_example(): pass\n",
    );
    open(&mut server, uri.clone(), source);

    let response = server.request::<HoverRequest>(hover_params(uri, Position::new(2, 30)));

    assert_json_snapshot!(workspace.normalize(response), @r###"
    {
      "contents": {
        "kind": "plaintext",
        "value": "def database():\n\nKarva fixture: database\nScope: function\nAutouse: false\nProvider: /project/conftest.py"
      },
      "range": {
        "end": {
          "character": 34,
          "line": 2
        },
        "start": {
          "character": 26,
          "line": 2
        }
      }
    }
    "###);
}

#[test]
fn hovers_nearest_nested_fixture_provider() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n\n@fixture\ndef database(): pass\n",
    );
    workspace.write(
        "package/conftest.py",
        "from karva import fixture\n\n@fixture\ndef database(): pass\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("package/test_example.py");
    open(
        &mut server,
        uri.clone(),
        "def test_example(database): pass\n",
    );

    let response = server.request::<HoverRequest>(hover_params(uri, Position::new(0, 20)));
    let response = workspace.normalize(response);

    assert_eq!(
        response["contents"]["value"],
        "def database():\n\nKarva fixture: database\nScope: function\nAutouse: false\nProvider: /project/package/conftest.py"
    );
}

#[test]
fn returns_no_hover_for_dynamic_fixture_provider() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    let source = concat!(
        "from karva import fixture\n\n",
        "fixture_name = get_fixture_name()\n",
        "@fixture(name=fixture_name)\n",
        "def dynamic(): pass\n\n",
        "def test_example(dynamic): pass\n",
    );
    open(&mut server, uri.clone(), source);

    let response = server.request::<HoverRequest>(hover_params(uri, Position::new(6, 19)));

    assert_eq!(response, None);
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

fn hover_params(uri: Uri, position: Position) -> HoverParams {
    HoverParams::new(
        WorkDoneProgressParams::default(),
        TextDocumentPositionParams::new(TextDocumentIdentifier::new(uri), position),
    )
}
