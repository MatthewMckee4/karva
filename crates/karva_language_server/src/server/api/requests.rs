use lsp_types::ShutdownRequest;

use super::traits::{RequestHandler, SyncRequestHandler};
use crate::session::Session;
use crate::session::client::Client;

mod completion;
mod definition;
mod hover;

pub(super) use completion::Completion;
pub(super) use definition::Definition;
pub(super) use hover::Hover;

pub(super) struct Shutdown;

impl RequestHandler for Shutdown {
    type RequestType = ShutdownRequest;
}

impl SyncRequestHandler for Shutdown {
    fn run(session: &mut Session, _client: &Client, (): ()) -> anyhow::Result<()> {
        session.request_shutdown();
        Ok(())
    }
}
