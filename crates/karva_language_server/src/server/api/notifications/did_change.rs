use lsp_types::{
    DidChangeTextDocumentNotification, DidChangeTextDocumentParams, TextDocumentIdentifier,
    VersionedTextDocumentIdentifier,
};

use super::super::traits::{NotificationHandler, SyncNotificationHandler};
use crate::session::Session;

pub(in crate::server::api) struct DidChange;

impl NotificationHandler for DidChange {
    type NotificationType = DidChangeTextDocumentNotification;
}

impl SyncNotificationHandler for DidChange {
    fn run(
        session: &mut Session,
        DidChangeTextDocumentParams {
            text_document:
                VersionedTextDocumentIdentifier {
                    text_document_identifier: TextDocumentIdentifier { uri },
                    version,
                },
            content_changes,
        }: DidChangeTextDocumentParams,
    ) -> anyhow::Result<()> {
        session.update_document(&uri, content_changes, version)?;
        Ok(())
    }
}
