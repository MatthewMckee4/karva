use lsp_types::{Notification, Request};

use crate::session::client::Client;
use crate::session::{DocumentSnapshotVersion, RequestCancellationToken, Session};

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
        cancellation: &RequestCancellationToken,
    ) -> anyhow::Result<<<Self as RequestHandler>::RequestType as Request>::Result>;

    fn document_version(_snapshot: &Self::Snapshot) -> Option<DocumentSnapshotVersion> {
        None
    }
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
