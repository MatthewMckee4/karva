use lsp_types::ShutdownRequest;

use super::traits::{RequestHandler, SyncRequestHandler};
use crate::session::Session;

pub(super) struct Shutdown;

impl RequestHandler for Shutdown {
    type RequestType = ShutdownRequest;
}

impl SyncRequestHandler for Shutdown {
    fn run(session: &mut Session, (): ()) -> anyhow::Result<()> {
        session.request_shutdown();
        Ok(())
    }
}
