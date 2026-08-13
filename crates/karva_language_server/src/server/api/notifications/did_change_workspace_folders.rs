use lsp_types::{
    DidChangeWorkspaceFoldersNotification, DidChangeWorkspaceFoldersParams,
    WorkspaceFoldersChangeEvent,
};

use super::super::traits::{NotificationHandler, SyncNotificationHandler};
use crate::session::Session;

pub struct DidChangeWorkspaceFolders;

impl NotificationHandler for DidChangeWorkspaceFolders {
    type NotificationType = DidChangeWorkspaceFoldersNotification;
}

impl SyncNotificationHandler for DidChangeWorkspaceFolders {
    fn run(
        session: &mut Session,
        DidChangeWorkspaceFoldersParams {
            event: WorkspaceFoldersChangeEvent { added, removed },
        }: DidChangeWorkspaceFoldersParams,
    ) -> anyhow::Result<()> {
        for folder in added {
            session.open_workspace_folder(folder);
        }
        for folder in removed {
            session.close_workspace_folder(&folder.uri)?;
        }
        Ok(())
    }
}
