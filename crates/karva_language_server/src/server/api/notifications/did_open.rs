use lsp_types::{DidOpenTextDocumentNotification, DidOpenTextDocumentParams, TextDocumentItem};

use super::super::diagnostics::publish_diagnostics;
use super::super::traits::{NotificationHandler, SyncNotificationHandler};
use crate::TextDocument;
use crate::session::Session;
use crate::session::client::Client;

pub struct DidOpen;

impl NotificationHandler for DidOpen {
    type NotificationType = DidOpenTextDocumentNotification;
}

impl SyncNotificationHandler for DidOpen {
    fn run(
        session: &mut Session,
        client: &Client,
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
        publish_diagnostics(session, client)
    }
}
