use lsp_server as lsp;

/// A connection builder that performs the LSP initialization handshake.
pub struct ConnectionInitializer {
    connection: lsp::Connection,
}

impl ConnectionInitializer {
    /// Creates a language-server connection over standard input and output.
    pub(crate) fn stdio() -> (Self, lsp::IoThreads) {
        let (connection, threads) = lsp::Connection::stdio();
        (Self { connection }, threads)
    }

    /// Creates an in-memory server and client connection pair.
    pub fn memory() -> (Self, lsp::Connection) {
        let (server, client) = lsp::Connection::memory();
        (Self { connection: server }, client)
    }

    pub(super) fn initialize_start(
        &self,
    ) -> anyhow::Result<(lsp::RequestId, lsp_types::InitializeParams)> {
        let (id, params) = self.connection.initialize_start()?;
        Ok((id, serde_json::from_value(params)?))
    }

    pub(super) fn initialize_finish(
        self,
        id: lsp::RequestId,
        server_capabilities: &lsp_types::ServerCapabilities,
        name: &str,
        version: &str,
    ) -> anyhow::Result<lsp::Connection> {
        self.connection.initialize_finish(
            id,
            serde_json::json!({
                "capabilities": server_capabilities,
                "serverInfo": {
                    "name": name,
                    "version": version,
                },
            }),
        )?;
        Ok(self.connection)
    }
}
