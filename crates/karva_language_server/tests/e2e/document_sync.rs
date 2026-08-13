use lsp_types::{
    DidChangeTextDocumentNotification, DidChangeTextDocumentParams,
    DidCloseTextDocumentNotification, DidCloseTextDocumentParams, DidOpenTextDocumentNotification,
    DidOpenTextDocumentParams, LanguageKind, Position, Range, TextDocumentContentChangeEvent,
    TextDocumentContentChangePartial, TextDocumentIdentifier, TextDocumentItem, Uri,
    VersionedTextDocumentIdentifier,
};

use super::TestServer;

#[test]
fn accepts_incremental_unsaved_document_changes() {
    let mut server = TestServer::new(lsp_types::ClientCapabilities::default());
    let uri = Uri::parse("file:///workspace/test_example.py").expect("test URI should be valid");
    server.notify::<DidOpenTextDocumentNotification>(DidOpenTextDocumentParams {
        text_document: TextDocumentItem {
            uri: uri.clone(),
            language_id: LanguageKind::Python,
            version: 1,
            text: "def test_😀():\n    pas\n".to_owned(),
        },
    });
    server.receive_notification::<lsp_types::PublishDiagnosticsNotification>();

    server.notify::<DidChangeTextDocumentNotification>(DidChangeTextDocumentParams {
        text_document: VersionedTextDocumentIdentifier {
            text_document_identifier: TextDocumentIdentifier { uri: uri.clone() },
            version: 2,
        },
        content_changes: vec![
            TextDocumentContentChangeEvent::TextDocumentContentChangePartial(
                TextDocumentContentChangePartial {
                    range: Range::new(Position::new(1, 7), Position::new(1, 7)),
                    text: "s".to_owned(),
                    ..TextDocumentContentChangePartial::default()
                },
            ),
        ],
    });
    server.receive_notification::<lsp_types::PublishDiagnosticsNotification>();

    server.notify::<DidCloseTextDocumentNotification>(DidCloseTextDocumentParams {
        text_document: TextDocumentIdentifier { uri },
    });
    server.receive_notification::<lsp_types::PublishDiagnosticsNotification>();
}
