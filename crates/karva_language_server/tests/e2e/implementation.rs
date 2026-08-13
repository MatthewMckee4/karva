use insta::assert_json_snapshot;
use lsp_types::{
    ClientCapabilities, DidChangeTextDocumentNotification, DidChangeTextDocumentParams,
    DidOpenTextDocumentNotification, DidOpenTextDocumentParams, ImplementationParams,
    ImplementationRequest, LanguageKind, PartialResultParams, Position,
    PublishDiagnosticsNotification, TextDocumentContentChangeEvent,
    TextDocumentContentChangeWholeDocument, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, Uri, VersionedTextDocumentIdentifier, WorkDoneProgressParams,
};

use super::{TestServer, Workspace};

#[test]
fn navigates_to_generator_and_return_fixture_implementations() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture\ndef database():\n    yield object()\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    let source = concat!(
        "import pytest\n",
        "from karva import fixture\n\n",
        "@fixture\n",
        "def local(): return object()\n\n",
        "@pytest.mark.usefixtures(\"database\")\n",
        "def test_example(database, local, tmp_path): pass\n",
    );
    open(&mut server, uri.clone(), source);

    let parameter =
        server.request::<ImplementationRequest>(params(uri.clone(), Position::new(7, 21)));
    let metadata =
        server.request::<ImplementationRequest>(params(uri.clone(), Position::new(6, 30)));
    let local = server.request::<ImplementationRequest>(params(uri.clone(), Position::new(7, 31)));
    let builtin = server.request::<ImplementationRequest>(params(uri, Position::new(7, 39)));

    assert_json_snapshot!(workspace.normalize([parameter, metadata, local, builtin]));
}

#[test]
fn rejects_an_implementation_from_a_stale_document_version() {
    let workspace = Workspace::new();
    workspace.write(
        "conftest.py",
        "from karva import fixture\n@fixture\ndef database():\n    yield object()\n",
    );
    let mut server = TestServer::with_workspace(ClientCapabilities::default(), workspace.folder());
    let uri = workspace.uri("test_example.py");
    open(
        &mut server,
        uri.clone(),
        "def test_example(database): pass\n",
    );

    let id =
        server.send_request::<ImplementationRequest>(params(uri.clone(), Position::new(0, 20)));
    server.notify::<DidChangeTextDocumentNotification>(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            text_document_identifier: TextDocumentIdentifier::new(uri),
            version: 2,
        },
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument {
                    text: "def test_example(other): pass\n".to_owned(),
                },
            ),
        ],
    });

    let response = server.receive_response(&id);
    server.receive_notification::<PublishDiagnosticsNotification>();
    let error = response
        .response_result
        .expect_err("stale implementation should fail");
    assert_eq!(error.code, lsp_server::ErrorCode::ContentModified as i32);
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

fn params(uri: Uri, position: Position) -> ImplementationParams {
    ImplementationParams::new(
        WorkDoneProgressParams::default(),
        PartialResultParams::default(),
        TextDocumentPositionParams::new(TextDocumentIdentifier::new(uri), position),
    )
}
