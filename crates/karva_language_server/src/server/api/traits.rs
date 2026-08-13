use lsp_types::{Notification, Request};

use crate::session::Session;
use crate::session::client::Client;

pub(super) trait RequestHandler {
    type RequestType: Request;
}

pub(super) trait SyncRequestHandler: RequestHandler {
    fn run(
        session: &mut Session,
        params: <<Self as RequestHandler>::RequestType as Request>::Params,
    ) -> anyhow::Result<<<Self as RequestHandler>::RequestType as Request>::Result>;
}

pub(super) trait NotificationHandler {
    type NotificationType: Notification;
}

pub(super) trait SyncNotificationHandler: NotificationHandler {
    fn run(
        session: &mut Session,
        client: &Client,
        params: <<Self as NotificationHandler>::NotificationType as Notification>::Params,
    ) -> anyhow::Result<()>;
}
