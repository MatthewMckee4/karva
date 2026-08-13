use lsp_types::{
    DidCloseTextDocumentNotification, DidCloseTextDocumentParams, TextDocumentIdentifier,
};

use super::super::diagnostics::publish_diagnostics;
use super::super::traits::{NotificationHandler, SyncNotificationHandler};
use crate::session::Session;
use crate::session::client::Client;

pub struct DidClose;

impl NotificationHandler for DidClose {
    type NotificationType = DidCloseTextDocumentNotification;
}

impl SyncNotificationHandler for DidClose {
    fn run(
        session: &mut Session,
        client: &Client,
        DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier { uri },
        }: DidCloseTextDocumentParams,
    ) -> anyhow::Result<()> {
        session.close_document(&uri)?;
        publish_diagnostics(session, client)
    }
}
