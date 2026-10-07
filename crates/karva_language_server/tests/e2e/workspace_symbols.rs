use lsp_types::{
    ClientCapabilities, DidOpenTextDocumentNotification, DidOpenTextDocumentParams, LanguageKind,
    PublishDiagnosticsNotification, TextDocumentItem, WorkspaceSymbolParams,
    WorkspaceSymbolRequest,
};

use super::{TestServer, TestServerBuilder, Workspace};

#[test]
fn searches_saved_and_unsaved_symbols_with_relaxed_queries() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture(name='данные')\ndef provider(): pass\n",
    );
    workspace.write("test_saved.py", "def test_saved(): pass\n");
    workspace.write("test_overlay.py", "def test_replaced(): pass\n");
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    assert!(
        server
            .initialization_result()
            .capabilities
            .workspace_symbol_provider
            .is_some()
    );
    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: workspace.uri("test_overlay.py"), language_id: LanguageKind::Python, version: 1,
            text: "from karva import fixture\n@fixture(name='😀данные')\ndef provider(): pass\ndef test_unsaved(): pass\n".to_owned(),
        },
    });
    server.receive_notification::<PublishDiagnosticsNotification>();
    let response = server.request::<WorkspaceSymbolRequest>(WorkspaceSymbolParams {
        query: String::new(),
        ..WorkspaceSymbolParams::default()
    });
    insta::assert_json_snapshot!(workspace.normalize(response), @r#"
    [
      {
        "containerName": "conftest.py",
        "kind": 12,
        "location": {
          "range": {
            "end": {
              "character": 21,
              "line": 1
            },
            "start": {
              "character": 15,
              "line": 1
            }
          },
          "uri": "file:///project/conftest.py"
        },
        "name": "данные"
      },
      {
        "containerName": "test_overlay.py",
        "kind": 12,
        "location": {
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
          "uri": "file:///project/test_overlay.py"
        },
        "name": "😀данные"
      },
      {
        "containerName": "test_overlay.py",
        "kind": 12,
        "location": {
          "range": {
            "end": {
              "character": 16,
              "line": 3
            },
            "start": {
              "character": 4,
              "line": 3
            }
          },
          "uri": "file:///project/test_overlay.py"
        },
        "name": "test_unsaved"
      },
      {
        "containerName": "test_saved.py",
        "kind": 12,
        "location": {
          "range": {
            "end": {
              "character": 14,
              "line": 0
            },
            "start": {
              "character": 4,
              "line": 0
            }
          },
          "uri": "file:///project/test_saved.py"
        },
        "name": "test_saved"
      }
    ]
    "#);
    let response = server.request::<WorkspaceSymbolRequest>(WorkspaceSymbolParams {
        query: "TUNSV".to_owned(),
        ..WorkspaceSymbolParams::default()
    });
    insta::assert_json_snapshot!(workspace.normalize(response), @r#"
    [
      {
        "containerName": "test_overlay.py",
        "kind": 12,
        "location": {
          "range": {
            "end": {
              "character": 16,
              "line": 3
            },
            "start": {
              "character": 4,
              "line": 3
            }
          },
          "uri": "file:///project/test_overlay.py"
        },
        "name": "test_unsaved"
      }
    ]
    "#);
}

#[test]
fn nested_workspaces_use_nearest_configuration_without_duplicates() {
    let workspace = Workspace::new();
    workspace.write("test_root.py", "def test_same(): pass\n");
    workspace.write(
        "nested/karva.toml",
        "[profile.default.test]\ntest-function-prefix = 'check'\n",
    );
    workspace.write(
        "nested/test_nested.py",
        "def test_hidden(): pass\ndef check_same(): pass\n",
    );
    let mut nested = workspace.folder();
    nested.uri = workspace.uri("nested");
    let mut server = TestServerBuilder::new()
        .with_workspace(workspace.folder())
        .with_workspace(nested)
        .build();
    let response = server.request::<WorkspaceSymbolRequest>(WorkspaceSymbolParams::default());
    insta::assert_json_snapshot!(workspace.normalize(response), @r#"
    [
      {
        "containerName": "test_nested.py",
        "kind": 12,
        "location": {
          "range": {
            "end": {
              "character": 14,
              "line": 1
            },
            "start": {
              "character": 4,
              "line": 1
            }
          },
          "uri": "file:///project/nested/test_nested.py"
        },
        "name": "check_same"
      },
      {
        "containerName": "test_root.py",
        "kind": 12,
        "location": {
          "range": {
            "end": {
              "character": 13,
              "line": 0
            },
            "start": {
              "character": 4,
              "line": 0
            }
          },
          "uri": "file:///project/test_root.py"
        },
        "name": "test_same"
      }
    ]
    "#);
}

#[rstest::rstest]
fn searches_independent_roots_and_preserves_duplicate_names(
    #[values("", "SAME", "absent")] query: &str,
) {
    let first = Workspace::new();
    let second = Workspace::new();
    first.write("test_a.py", "def test_same(): pass\n");
    second.write("test_b.py", "def test_same(): pass\n");
    let mut server = TestServerBuilder::new()
        .with_workspace(first.folder())
        .with_workspace(second.folder())
        .build();
    let response = server.request::<WorkspaceSymbolRequest>(WorkspaceSymbolParams {
        query: query.to_owned(),
        ..WorkspaceSymbolParams::default()
    });
    let value = serde_json::to_value(response).expect("symbols serialize");
    let symbols = value.as_array().expect("symbol list");
    if query == "absent" {
        assert!(symbols.is_empty());
    } else {
        assert_eq!(symbols.len(), 2);
        assert!(symbols.iter().all(|symbol| symbol["name"] == "test_same"));
        let locations = symbols
            .iter()
            .map(|symbol| symbol["location"]["uri"].as_str().expect("URI"))
            .collect::<Vec<_>>();
        assert!(locations.contains(&first.uri("test_a.py").as_str()));
        assert!(locations.contains(&second.uri("test_b.py").as_str()));
    }
}
