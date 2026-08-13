use insta::assert_json_snapshot;
use lsp_types::{
    ClientCapabilities, DidChangeTextDocumentNotification, DidChangeTextDocumentParams,
    DidOpenTextDocumentNotification, DidOpenTextDocumentParams, LanguageKind,
    PublishDiagnosticsNotification, TextDocumentContentChangeEvent,
    TextDocumentContentChangeWholeDocument, TextDocumentIdentifier, TextDocumentItem,
    VersionedTextDocumentIdentifier,
};
use serde_json::{Value, json};

use super::{TestServer, TestServerBuilder, Workspace};

const METHOD: &str = "experimental/runnables";
const SOURCE: &str = concat!(
    "import karva\n",
    "from karva import fixture\n\n",
    "@fixture\n",
    "def database(): pass\n\n",
    "def helper(): pass\n\n",
    "@karva.tags.parametrize(\"value\", [1, 2])\n",
    "def test_static(value): pass\n\n",
    "def test_dynamic(): pass\n",
);

#[test]
fn returns_project_file_test_and_static_case_runnables() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("tests/test_runnables.py");
    workspace.write("tests/test_runnables.py", "");
    open(&mut server, uri.clone(), SOURCE);

    let response = request(&mut server, &workspace, uri, None);

    assert_json_snapshot!(response);
}

#[test]
fn uses_nested_project_root_for_cwd_and_relative_selector() {
    let workspace = Workspace::new();
    workspace.write(
        "nested/karva.toml",
        "[profile.default.test]\ntest-function-prefix = \"check_\"\n",
    );
    let uri = workspace.uri("nested/tests/test_nested.py");
    workspace.write("nested/tests/test_nested.py", "");
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    open(&mut server, uri.clone(), "def check_nested(): pass\n");

    let response = request(&mut server, &workspace, uri, None);

    assert_eq!(response[0]["args"]["cwd"], "/project/nested");
    assert_eq!(response[0]["args"]["args"], json!(["run", "karva", "test"]));
    assert_eq!(
        response[1]["args"]["args"],
        json!(["run", "karva", "test", "tests/test_nested.py"])
    );
    assert_eq!(
        response[2]["args"]["args"],
        json!(["run", "karva", "test", "tests/test_nested.py::check_nested"])
    );
}

#[test]
fn keeps_runnables_independent_across_workspace_roots() {
    let first = Workspace::new();
    first.write(
        "karva.toml",
        "[profile.default.test]\ntest-function-prefix = \"first_\"\n",
    );
    let first_uri = first.uri("tests/test_first.py");
    first.write("tests/test_first.py", "");

    let second = Workspace::new();
    second.write(
        "karva.toml",
        "[profile.default.test]\ntest-function-prefix = \"second_\"\n",
    );
    let second_uri = second.uri("tests/test_second.py");
    second.write("tests/test_second.py", "");

    let mut server = TestServerBuilder::new()
        .with_workspace(first.folder())
        .with_workspace(second.folder())
        .build();
    open(&mut server, first_uri.clone(), "def first_case(): pass\n");
    open(&mut server, second_uri.clone(), "def second_case(): pass\n");

    let first_response = request(&mut server, &first, first_uri, None);
    let second_response = request(&mut server, &second, second_uri, None);
    server.receive_notification::<PublishDiagnosticsNotification>();

    assert_eq!(first_response[0]["args"]["cwd"], "/project");
    assert_eq!(
        first_response[2]["args"]["args"],
        json!(["run", "karva", "test", "tests/test_first.py::first_case"])
    );
    assert_eq!(second_response[0]["args"]["cwd"], "/project");
    assert_eq!(
        second_response[2]["args"]["args"],
        json!(["run", "karva", "test", "tests/test_second.py::second_case"])
    );
}

#[test]
fn position_returns_only_the_containing_test() {
    let workspace = Workspace::new();
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("tests/test_runnables.py");
    workspace.write("tests/test_runnables.py", "");
    open(&mut server, uri.clone(), SOURCE);

    let response = request(
        &mut server,
        &workspace,
        uri,
        Some(json!({"line": 9, "character": 8})),
    );
    let labels = response
        .as_array()
        .expect("runnables response should be an array")
        .iter()
        .map(|runnable| {
            runnable["label"]
                .as_str()
                .expect("runnable should have a label")
        })
        .collect::<Vec<_>>();

    assert_eq!(
        labels,
        [
            "Run test_static",
            "Run test_static[0]",
            "Run test_static[1]"
        ]
    );
}

#[test]
fn uses_live_source_and_selected_profile() {
    let workspace = Workspace::new();
    let mut server = TestServerBuilder::new()
        .with_workspace(workspace.folder())
        .with_initialization_options(json!({"profile": "ci"}))
        .build();
    workspace.write(
        "karva.toml",
        "[profile.ci.test]\ntest-function-prefix = \"check_\"\n",
    );
    let uri = workspace.uri("tests/test_live.py");
    workspace.write("tests/test_live.py", "");
    open(&mut server, uri.clone(), "def check_before(): pass\n");
    server.notify::<DidChangeTextDocumentNotification>(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            text_document_identifier: TextDocumentIdentifier::new(uri.clone()),
            version: 2,
        },
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument {
                    text: "def check_after(): pass\n".to_owned(),
                },
            ),
        ],
    });
    server.receive_notification::<PublishDiagnosticsNotification>();

    let response = request(&mut server, &workspace, uri, None);
    let serialized = serde_json::to_string(&response).expect("response should serialize");

    assert!(serialized.contains("check_after"));
    assert!(!serialized.contains("check_before"));
    assert!(serialized.contains("--profile"));
    assert!(serialized.contains("ci"));
}

#[test]
fn returns_doctests_when_enabled_by_the_profile() {
    let workspace = Workspace::new();
    workspace.write(
        "karva.toml",
        "[profile.default.test]\ndoctest-modules = true\n",
    );
    workspace.write("tests/test_doctest.py", "");
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("tests/test_doctest.py");
    open(
        &mut server,
        uri.clone(),
        "\"\"\">>> 1 + 1\n2\n\"\"\"\n\nclass Example:\n    \"\"\">>> 2 + 2\n    4\n    \"\"\"\n",
    );

    let response = request(&mut server, &workspace, uri, None);
    let labels = response
        .as_array()
        .expect("runnables response should be an array")
        .iter()
        .map(|runnable| {
            runnable["label"]
                .as_str()
                .expect("runnable should have a label")
        })
        .collect::<Vec<_>>();

    assert_eq!(
        labels,
        [
            "Run Karva project",
            "Run Karva file",
            "Run doctest:@module",
            "Run doctest:Example"
        ]
    );
    assert_eq!(
        response[2]["args"]["args"],
        json!([
            "run",
            "karva",
            "test",
            "tests/test_doctest.py::doctest:@module"
        ])
    );
}

#[test]
fn cancels_a_background_runnable_request_once() {
    let workspace = Workspace::new();
    workspace.write("tests/test_cancel.py", "");
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("tests/test_cancel.py");
    open(&mut server, uri.clone(), "def test_cancel(): pass\n");

    let request_id = server.send_request_raw(METHOD, params(uri.clone(), None));
    server.cancel(&request_id);
    let response = server.receive_response(&request_id);
    let error = response
        .response_result
        .expect_err("cancelled runnable request should fail");

    assert_eq!(error.code, lsp_server::ErrorCode::RequestCanceled as i32);
    assert!(
        !request(&mut server, &workspace, uri, None)
            .as_array()
            .expect("runnables response should be an array")
            .is_empty()
    );
}

#[test]
fn rejects_runnables_from_a_stale_document_version() {
    let workspace = Workspace::new();
    workspace.write("tests/test_stale.py", "");
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("tests/test_stale.py");
    open(&mut server, uri.clone(), "def test_before(): pass\n");

    let request_id = server.send_request_raw(METHOD, params(uri.clone(), None));
    change(&server, uri.clone(), 2, "def test_after(): pass\n");
    let response = server.receive_response(&request_id);
    server.receive_notification::<PublishDiagnosticsNotification>();
    let error = response
        .response_result
        .expect_err("stale runnable request should fail");

    assert_eq!(error.code, lsp_server::ErrorCode::ContentModified as i32);
    let fresh = request(&mut server, &workspace, uri, None);
    let serialized = serde_json::to_string(&fresh).expect("response should serialize");
    assert!(serialized.contains("test_after"));
    assert!(!serialized.contains("test_before"));
}

#[test]
fn malformed_params_return_invalid_params() {
    let mut server = TestServer::new(ClientCapabilities::default());

    let response = server.request_raw(METHOD, json!({}));
    let error = response
        .response_result
        .expect_err("malformed runnable request should fail");

    assert_eq!(error.code, lsp_server::ErrorCode::InvalidParams as i32);
}

fn open(server: &mut TestServer, uri: lsp_types::Uri, source: &str) {
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

fn change(server: &TestServer, uri: lsp_types::Uri, version: i32, source: &str) {
    server.notify::<DidChangeTextDocumentNotification>(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            text_document_identifier: TextDocumentIdentifier::new(uri),
            version,
        },
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument {
                    text: source.to_owned(),
                },
            ),
        ],
    });
}

fn params(uri: lsp_types::Uri, position: Option<Value>) -> Value {
    json!({
        "textDocument": {"uri": uri},
        "position": position,
    })
}

fn request(
    server: &mut TestServer,
    workspace: &Workspace,
    uri: lsp_types::Uri,
    position: Option<Value>,
) -> Value {
    let response = server.request_raw(METHOD, params(uri, position));
    let result = response
        .response_result
        .unwrap_or_else(|error| panic!("runnables request failed: {error:?}"));
    workspace.normalize(result)
}
