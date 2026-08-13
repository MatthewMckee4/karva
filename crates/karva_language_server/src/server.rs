//! Scheduling, I/O, and API endpoints.

use lsp_server::Connection;
use lsp_types::InitializeParams;

use crate::capabilities::{position_encoding, server_capabilities};
use crate::session::Session;

pub use self::connection::ConnectionInitializer;

mod api;
mod connection;
mod main_loop;

/// An initialized Karva language server.
pub struct Server {
    connection: Connection,
    session: Session,
}

impl Server {
    /// Completes the LSP initialization handshake.
    pub fn new(connection: ConnectionInitializer) -> anyhow::Result<Self> {
        let (id, init_params) = connection.initialize_start()?;
        let InitializeParams { capabilities, .. } = init_params;
        let capabilities = server_capabilities(position_encoding(&capabilities));
        let connection = connection.initialize_finish(
            id,
            &capabilities,
            crate::SERVER_NAME,
            karva_version::version(),
        )?;

        Ok(Self {
            connection,
            session: Session::default(),
        })
    }

    /// Handles client messages until the LSP shutdown sequence completes.
    pub fn run(mut self) -> anyhow::Result<()> {
        self.main_loop()
    }
}
