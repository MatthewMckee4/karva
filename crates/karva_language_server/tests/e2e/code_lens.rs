use lsp_types::{
    ClientCapabilities, CodeLensParams, CodeLensRequest, DidCloseTextDocumentNotification,
    DidCloseTextDocumentParams, DidOpenTextDocumentNotification, DidOpenTextDocumentParams,
    LanguageKind, PartialResultParams, PublishDiagnosticsNotification, TextDocumentIdentifier,
    TextDocumentItem, Uri, WorkDoneProgressParams,
};
use serde_json::json;

use super::{TestServer, TestServerBuilder, Workspace};

fn open(server: &mut TestServer, uri: Uri, text: &str) {
    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri,
            language_id: LanguageKind::Python,
            version: 1,
            text: text.to_owned(),
        },
    });
    server.receive_notification::<PublishDiagnosticsNotification>();
}

fn params(uri: Uri) -> CodeLensParams {
    CodeLensParams::new(
        TextDocumentIdentifier::new(uri),
        WorkDoneProgressParams::default(),
        PartialResultParams::default(),
    )
}

#[test]
fn test_commands_use_project_root_profile_and_runtime_identity() {
    let workspace = Workspace::new();
    workspace.write(
        "nested/karva.toml",
        "[profile.ci.test]\ntest-function-prefix = 'check'\n",
    );
    let mut builder = TestServerBuilder::new().with_workspace(workspace.folder());
    builder.initialize_params.initialization_options = Some(json!({"profile": "ci"}));
    let mut server = builder.build();
    let uri = workspace.uri("nested/test space.py");
    open(
        &mut server,
        uri.clone(),
        "raise RuntimeError('must never import')\nfrom karva import fixture\n@fixture\ndef provider(): pass\ndef helper(): pass\ndef test_ignored(): pass\ndef check_𐐀case(): pass\n",
    );
    assert!(
        server
            .initialization_result()
            .capabilities
            .code_lens_provider
            .is_some()
    );
    let response = server.request::<CodeLensRequest>(params(uri));
    insta::assert_json_snapshot!(workspace.normalize(response), @r#"
    [
      {
        "command": {
          "arguments": [
            {
              "args": [
                "run",
                "karva",
                "test",
                "test space.py::check_𐐀case",
                "--profile=ci"
              ],
              "cwd": "/project/nested",
              "program": "uv"
            }
          ],
          "command": "karva.runTests",
          "title": "Run test"
        },
        "range": {
          "end": {
            "character": 16,
            "line": 6
          },
          "start": {
            "character": 4,
            "line": 6
          }
        }
      },
      {
        "command": {
          "arguments": [
            {
              "args": [
                "run",
                "karva",
                "test",
                "test space.py",
                "--profile=ci"
              ],
              "cwd": "/project/nested",
              "program": "uv"
            }
          ],
          "command": "karva.runTests",
          "title": "Run file"
        },
        "range": {
          "end": {
            "character": 16,
            "line": 6
          },
          "start": {
            "character": 4,
            "line": 6
          }
        }
      },
      {
        "command": {
          "arguments": [
            "test space::check_𐐀case"
          ],
          "command": "karva.copyTestId",
          "title": "Copy test ID"
        },
        "range": {
          "end": {
            "character": 16,
            "line": 6
          },
          "start": {
            "character": 4,
            "line": 6
          }
        }
      }
    ]
    "#);
}

#[test]
fn file_moves_and_unsaved_names_update_selectors() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    workspace.write("tests/test_old.py", "def test_old(): pass\n");
    let old = workspace.uri("tests/test_old.py");
    open(&mut server, old.clone(), "def test_old(): pass\n");
    server.notify::<DidCloseTextDocumentNotification>(DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier::new(old),
    });
    server.receive_notification::<PublishDiagnosticsNotification>();
    let new = workspace.uri("tests/test_new.py");
    open(&mut server, new.clone(), "def test_new(): pass\n");
    let response = server.request::<CodeLensRequest>(params(new));
    let value = workspace.normalize(response);
    assert_eq!(
        value[0]["command"]["arguments"][0]["args"][3],
        "tests/test_new.py::test_new"
    );
    assert_eq!(
        value[2]["command"]["arguments"][0],
        "tests.test_new::test_new"
    );
}

#[rstest::rstest]
fn helpers_and_fixtures_have_no_test_lenses(
    #[values(
        "def helper(): pass\n",
        "from karva import fixture\n@fixture\ndef provider(): pass\n"
    )]
    source: &str,
) {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    open(&mut server, uri.clone(), source);
    let response = server.request::<CodeLensRequest>(params(uri));
    assert_eq!(
        serde_json::to_value(response).expect("lenses serialize"),
        json!([])
    );
}
