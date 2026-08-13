use lsp_types::{DidChangeWatchedFilesNotification, DidChangeWatchedFilesParams};

use super::super::traits::{NotificationHandler, SyncNotificationHandler};
use crate::session::Session;

pub struct DidChangeWatchedFiles;

impl NotificationHandler for DidChangeWatchedFiles {
    type NotificationType = DidChangeWatchedFilesNotification;
}

impl SyncNotificationHandler for DidChangeWatchedFiles {
    fn run(
        session: &mut Session,
        DidChangeWatchedFilesParams { changes }: DidChangeWatchedFilesParams,
    ) -> anyhow::Result<()> {
        for change in changes {
            session.configuration_changed(&change.uri)?;
        }
        Ok(())
    }
}
