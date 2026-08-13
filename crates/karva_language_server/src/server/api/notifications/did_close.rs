use lsp_types::{
    DidCloseTextDocumentNotification, DidCloseTextDocumentParams, TextDocumentIdentifier,
};

use super::super::traits::{DiagnosticNotificationHandler, NotificationHandler};
use crate::session::Session;

pub struct DidClose;

impl NotificationHandler for DidClose {
    type NotificationType = DidCloseTextDocumentNotification;
}

impl DiagnosticNotificationHandler for DidClose {
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
