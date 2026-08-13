use lsp_types::{DidChangeWatchedFilesNotification, DidChangeWatchedFilesParams};

use super::super::diagnostics::publish_diagnostics;
use super::super::traits::{NotificationHandler, SyncNotificationHandler};
use crate::session::Session;
use crate::session::client::Client;

pub(in crate::server::api) struct DidChangeWatchedFiles;

impl NotificationHandler for DidChangeWatchedFiles {
    type NotificationType = DidChangeWatchedFilesNotification;
}

impl SyncNotificationHandler for DidChangeWatchedFiles {
    fn run(
        session: &mut Session,
        client: &Client,
        DidChangeWatchedFilesParams { changes }: DidChangeWatchedFilesParams,
    ) -> anyhow::Result<()> {
        for change in changes {
            session.configuration_changed(&change.uri)?;
        }
        publish_diagnostics(session, client)
    }
}
