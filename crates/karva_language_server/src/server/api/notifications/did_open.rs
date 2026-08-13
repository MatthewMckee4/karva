use lsp_types::{DidOpenTextDocumentNotification, DidOpenTextDocumentParams, TextDocumentItem};

use super::super::traits::{NotificationHandler, SyncNotificationHandler};
use crate::TextDocument;
use crate::session::Session;

pub(in crate::server::api) struct DidOpen;

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
        let document = TextDocument::new(uri, text, version, language_id);
        if let Err(error) = session.project_for_uri(document.uri()) {
            tracing::warn!("failed to resolve Karva project: {error}");
        }
        session.open_document(document);
        Ok(())
    }
}
