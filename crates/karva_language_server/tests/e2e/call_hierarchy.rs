use lsp_types::{
    CallHierarchyIncomingCallsParams, CallHierarchyIncomingCallsRequest, CallHierarchyItem,
    CallHierarchyOutgoingCallsParams, CallHierarchyOutgoingCallsRequest,
    CallHierarchyPrepareParams, CallHierarchyPrepareRequest, ClientCapabilities,
    DidChangeTextDocumentNotification, DidChangeTextDocumentParams,
    DidOpenTextDocumentNotification, DidOpenTextDocumentParams, LanguageKind, PartialResultParams,
    Position, PublishDiagnosticsNotification, TextDocumentContentChangeEvent,
    TextDocumentContentChangeWholeDocument, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, VersionedTextDocumentIdentifier, WorkDoneProgressParams,
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

fn prepare(
    server: &mut TestServer,
    uri: Uri,
    position: Position,
) -> Option<Vec<CallHierarchyItem>> {
    server.request::<CallHierarchyPrepareRequest>(CallHierarchyPrepareParams::new(
        WorkDoneProgressParams::default(),
        TextDocumentPositionParams::new(TextDocumentIdentifier::new(uri), position),
    ))
}

fn incoming(item: CallHierarchyItem) -> CallHierarchyIncomingCallsParams {
    CallHierarchyIncomingCallsParams::new(
        item,
        WorkDoneProgressParams::default(),
        PartialResultParams::default(),
    )
}

fn outgoing(item: CallHierarchyItem) -> CallHierarchyOutgoingCallsParams {
    CallHierarchyOutgoingCallsParams::new(
        item,
        WorkDoneProgressParams::default(),
        PartialResultParams::default(),
    )
}

#[test]
fn navigates_unopened_providers_and_unsaved_consumers() {
    let workspace = Workspace::new();
    workspace.write("conftest.py", "from karva import fixture\n@fixture\ndef database(): pass\n@fixture\ndef wrapper(database): pass\n");
    workspace.write("test_saved.py", "def test_saved(wrapper): pass\n");
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("tests/test_unsaved.py");
    open(
        &mut server,
        uri.clone(),
        "import pytest\n@pytest.mark.usefixtures('wrapper')\ndef test_unsaved(wrapper): pass\n",
    );
    let item = prepare(&mut server, uri, Position::new(2, 20))
        .expect("fixture item")
        .remove(0);
    assert_eq!(item.name, "wrapper (fixture)");
    assert_eq!(item.uri, workspace.uri("conftest.py"));
    assert_eq!(item.selection_range.start, Position::new(4, 4));
    let consumers = server
        .request::<CallHierarchyIncomingCallsRequest>(incoming(item.clone()))
        .expect("incoming calls");
    assert_eq!(
        consumers
            .iter()
            .map(|call| (call.from.name.as_str(), call.from_ranges.len()))
            .collect::<Vec<_>>(),
        [("test_saved", 1), ("test_unsaved", 2)]
    );
    assert!(consumers.iter().all(|call| {
        call.from_ranges
            .iter()
            .all(|range| range.start >= call.from.range.start && range.end <= call.from.range.end)
    }));
    let dependencies = server
        .request::<CallHierarchyOutgoingCallsRequest>(outgoing(item))
        .expect("outgoing calls");
    assert_eq!(dependencies.len(), 1);
    assert_eq!(dependencies[0].to.name, "database (fixture)");
    assert_eq!(dependencies[0].from_ranges[0].start, Position::new(4, 12));
    // The origin is still open; neither provider file was opened by this test.
    let database_consumers = server
        .request::<CallHierarchyIncomingCallsRequest>(incoming(dependencies[0].to.clone()))
        .expect("recursive incoming calls");
    assert_eq!(database_consumers.len(), 1);
    assert_eq!(database_consumers[0].from.name, "wrapper (fixture)");
    let test_dependencies = server
        .request::<CallHierarchyOutgoingCallsRequest>(outgoing(consumers[1].from.clone()))
        .expect("test dependencies");
    assert_eq!(test_dependencies.len(), 1);
    assert_eq!(test_dependencies[0].from_ranges.len(), 2);
    insta::assert_json_snapshot!(workspace.normalize(dependencies), @r#"
    [
      {
        "fromRanges": [
          {
            "end": {
              "character": 20,
              "line": 4
            },
            "start": {
              "character": 12,
              "line": 4
            }
          }
        ],
        "to": {
          "data": "file:///project/tests/test_unsaved.py",
          "kind": 12,
          "name": "database (fixture)",
          "range": {
            "end": {
              "character": 20,
              "line": 2
            },
            "start": {
              "character": 0,
              "line": 1
            }
          },
          "selectionRange": {
            "end": {
              "character": 12,
              "line": 2
            },
            "start": {
              "character": 4,
              "line": 2
            }
          },
          "uri": "file:///project/conftest.py"
        }
      }
    ]
    "#);
}

#[test]
fn nested_overrides_resolve_to_the_correct_provider() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture\ndef database(): pass\n",
    );
    workspace.write(
        "pkg/conftest.py",
        "from karva import fixture\n@fixture\ndef database(database): pass\n",
    );
    workspace.write("test_root.py", "def test_root(database): pass\n");
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("pkg/test_nested.py");
    open(
        &mut server,
        uri.clone(),
        "def test_nested(database): pass\n",
    );
    let nested = prepare(&mut server, uri, Position::new(0, 20))
        .expect("nested provider")
        .remove(0);
    assert_eq!(nested.uri, workspace.uri("pkg/conftest.py"));
    let consumers = server
        .request::<CallHierarchyIncomingCallsRequest>(incoming(nested.clone()))
        .expect("incoming");
    assert_eq!(consumers.len(), 1);
    assert_eq!(consumers[0].from.name, "test_nested");
    let dependencies = server
        .request::<CallHierarchyOutgoingCallsRequest>(outgoing(nested))
        .expect("outgoing");
    assert_eq!(dependencies.len(), 1);
    assert_eq!(dependencies[0].to.uri, workspace.uri("conftest.py"));
    let root = server
        .request::<CallHierarchyIncomingCallsRequest>(incoming(dependencies[0].to.clone()))
        .expect("root consumers");
    assert_eq!(
        root.iter()
            .map(|call| call.from.name.as_str())
            .collect::<Vec<_>>(),
        ["database (fixture)", "test_root"]
    );
}

#[test]
fn unicode_ranges_and_custom_names_use_current_overlays() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_unicode.py");
    let source = "from karva import fixture\n@fixture(name='данные')\ndef provider(): pass\n@fixture\ndef wrapper(данные, tmp_path): pass\ndef test_example(wrapper): pass\n";
    open(&mut server, uri.clone(), source);
    let provider = prepare(&mut server, uri.clone(), Position::new(4, 15))
        .expect("provider")
        .remove(0);
    assert_eq!(provider.name, "provider (fixture)");
    assert_eq!(provider.selection_range.start, Position::new(2, 4));
    let calls = server
        .request::<CallHierarchyIncomingCallsRequest>(incoming(provider))
        .expect("incoming");
    assert_eq!(
        calls[0].from_ranges[0].end.character - calls[0].from_ranges[0].start.character,
        6
    );
    let wrapper = prepare(&mut server, uri.clone(), Position::new(5, 20))
        .expect("wrapper")
        .remove(0);
    let calls = server
        .request::<CallHierarchyOutgoingCallsRequest>(outgoing(wrapper.clone()))
        .expect("outgoing");
    assert_eq!(calls.len(), 1);
    server.notify::<DidChangeTextDocumentNotification>(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            text_document_identifier: TextDocumentIdentifier::new(uri.clone()),
            version: 2,
        },
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument {
                    text: source.replace("wrapper(данные, tmp_path)", "wrapper(tmp_path)"),
                },
            ),
        ],
    });
    server.receive_notification::<PublishDiagnosticsNotification>();
    // The changed function range invalidates the old item; no edges from stale coordinates.
    assert!(
        server
            .request::<CallHierarchyOutgoingCallsRequest>(outgoing(wrapper))
            .expect("stale calls")
            .is_empty()
    );
    let current = prepare(&mut server, uri, Position::new(5, 20))
        .expect("current wrapper")
        .remove(0);
    assert!(
        server
            .request::<CallHierarchyOutgoingCallsRequest>(outgoing(current))
            .expect("builtin excluded")
            .is_empty()
    );
}

#[rstest::rstest]
fn unsupported_selections_have_no_hierarchy(
    #[values(
        "def test_example(tmp_path): pass\n",
        "def test_example(missing): pass\n",
        "from karva import fixture\n@fixture(scope='invalid')\ndef rejected(): pass\ndef test_example(rejected): pass\n",
        "def helper(): pass\ndef test_example():\n    helper()\n"
    )]
    source: &str,
) {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    open(&mut server, uri.clone(), source);
    let line = if source.contains("rejected") {
        3
    } else if source.contains("helper") {
        2
    } else {
        0
    };
    let column = if source.contains("helper") { 5 } else { 20 };
    assert!(prepare(&mut server, uri, Position::new(line, column)).is_none());
}
