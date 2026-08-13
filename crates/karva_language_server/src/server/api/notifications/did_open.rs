use lsp_types::{DidOpenTextDocumentNotification, DidOpenTextDocumentParams, TextDocumentItem};

use super::super::traits::{NotificationHandler, SyncNotificationHandler};
use crate::TextDocument;
use crate::session::Session;

pub struct DidOpen;

impl NotificationHandler for DidOpen {
    type NotificationType = DidOpenTextDocumentNotification;
}

impl SyncNotificationHandler for DidOpen {
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
        session.open_document(TextDocument::new(uri, text, version, language_id));
        Ok(())
    }
}
