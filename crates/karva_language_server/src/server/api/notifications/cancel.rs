use lsp_server::RequestId;
use lsp_types::{CancelNotification, CancelParams, Id};

use super::super::traits::{NotificationHandler, SyncNotificationHandler};
use crate::session::Session;
use crate::session::client::Client;

pub struct Cancel;

impl NotificationHandler for Cancel {
    type NotificationType = CancelNotification;
}

impl SyncNotificationHandler for Cancel {
    fn run(
        session: &mut Session,
        client: &Client,
        CancelParams { id }: CancelParams,
    ) -> anyhow::Result<()> {
        let id = match id {
            Id::Int(id) => RequestId::from(id),
            Id::String(id) => RequestId::from(id),
        };
        client.cancel(session, id)
    }
}
