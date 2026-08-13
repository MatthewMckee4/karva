use lsp_types::ShutdownRequest;

use super::traits::{RequestHandler, SyncRequestHandler};
use crate::session::Session;
use crate::session::client::Client;

mod completion;
mod definition;
mod document_highlight;
mod document_symbols;
mod hover;
mod prepare_rename;
mod references;
mod rename;

pub(super) use completion::Completion;
pub(super) use definition::Definition;
pub(super) use document_highlight::DocumentHighlight;
pub(super) use document_symbols::DocumentSymbols;
pub(super) use hover::Hover;
pub(super) use prepare_rename::PrepareRename;
pub(super) use references::References;
pub(super) use rename::Rename;

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
