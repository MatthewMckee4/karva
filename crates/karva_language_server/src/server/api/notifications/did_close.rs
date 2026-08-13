use lsp_types::{
    DidCloseTextDocumentNotification, DidCloseTextDocumentParams, TextDocumentIdentifier,
};

use super::super::traits::{NotificationHandler, SyncNotificationHandler};
use crate::session::Session;

pub struct DidClose;

impl NotificationHandler for DidClose {
    type NotificationType = DidCloseTextDocumentNotification;
}

impl SyncNotificationHandler for DidClose {
    fn run(
        session: &mut Session,
        DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
        }: DidCloseTextDocumentParams,
    ) -> anyhow::Result<()> {
        session.close_document(&uri)?;
        Ok(())
    }
}
