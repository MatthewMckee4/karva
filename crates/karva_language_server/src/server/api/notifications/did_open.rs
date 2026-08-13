use lsp_types::{DidOpenTextDocumentNotification, DidOpenTextDocumentParams, TextDocumentItem};

use super::super::traits::{DiagnosticNotificationHandler, NotificationHandler};
use crate::TextDocument;
use crate::session::Session;

pub struct DidOpen;

impl NotificationHandler for DidOpen {
    type NotificationType = DidOpenTextDocumentNotification;
}

impl DiagnosticNotificationHandler for DidOpen {
    fn run(
        session: &mut Session,
        DidOpenTextDocumentParams {
            text_document:
                TextDocumentItem {
                    uri,
                    text,
                    version,
                    language_id,
                },
        }: DidOpenTextDocumentParams,
    ) -> anyhow::Result<()> {
        let document = TextDocument::new(uri, text, version, language_id);
        session.open_document(document);
        Ok(())
    }
}
