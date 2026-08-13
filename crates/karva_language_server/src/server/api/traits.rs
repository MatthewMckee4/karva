use lsp_types::{Notification, Request};

use crate::session::Session;
use crate::session::client::Client;

pub(super) trait RequestHandler {
    type RequestType: Request;
}

pub(super) trait SyncRequestHandler: RequestHandler {
    fn run(
        session: &mut Session,
        client: &Client,
        params: <<Self as RequestHandler>::RequestType as Request>::Params,
    ) -> anyhow::Result<<<Self as RequestHandler>::RequestType as Request>::Result>;
}

/// A request whose owned snapshot can run without blocking the event loop.
#[expect(
    dead_code,
    reason = "the first background feature handler lands in the next stacked layer"
)]
pub(super) trait BackgroundRequestHandler: RequestHandler {
    type Snapshot: Send + 'static;

    fn prepare(
        session: &mut Session,
        params: &<<Self as RequestHandler>::RequestType as Request>::Params,
    ) -> anyhow::Result<Self::Snapshot>;

    fn run(
        snapshot: Self::Snapshot,
        client: &Client,
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
