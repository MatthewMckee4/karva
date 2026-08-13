use lsp_types::{
    DidChangeWorkspaceFoldersNotification, DidChangeWorkspaceFoldersParams,
    WorkspaceFoldersChangeEvent,
};

use super::super::diagnostics::publish_diagnostics;
use super::super::traits::{NotificationHandler, SyncNotificationHandler};
use crate::session::Session;
use crate::session::client::Client;

pub(in crate::server::api) struct DidChangeWorkspaceFolders;

impl NotificationHandler for DidChangeWorkspaceFolders {
    type NotificationType = DidChangeWorkspaceFoldersNotification;
}

impl SyncNotificationHandler for DidChangeWorkspaceFolders {
    fn run(
        session: &mut Session,
        client: &Client,
        DidChangeWorkspaceFoldersParams {
            event: WorkspaceFoldersChangeEvent { added, removed },
        }: DidChangeWorkspaceFoldersParams,
    ) -> anyhow::Result<()> {
        for folder in added {
            session.open_workspace_folder(folder)?;
        }
        for folder in removed {
            session.close_workspace_folder(&folder.uri)?;
        }
        publish_diagnostics(session, client)
    }
}
