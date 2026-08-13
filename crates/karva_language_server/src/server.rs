//! Scheduling, I/O, and API endpoints.

use lsp_server::Connection;
use lsp_types::{InitializeParams, WorkspaceFolders, WorkspaceFoldersInitializeParams};
use serde::Deserialize;

use crate::capabilities::{position_encoding, server_capabilities};
use crate::session::Session;
use crate::workspace::Workspaces;

pub use self::connection::ConnectionInitializer;

mod api;
mod connection;
mod main_loop;

/// An initialized Karva language server.
pub struct Server {
    connection: Connection,
    session: Session,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct InitializationOptions {
    profile: Option<String>,
}

impl Server {
    /// Completes the LSP initialization handshake.
    pub fn new(connection: ConnectionInitializer) -> anyhow::Result<Self> {
        let (id, init_params) = connection.initialize_start()?;
        let InitializeParams {
            capabilities,
            initialization_options,
            workspace_folders_initialize_params:
                WorkspaceFoldersInitializeParams { workspace_folders },
            ..
        } = init_params;
        let position_encoding = position_encoding(&capabilities);
        let capabilities = server_capabilities(position_encoding);
        let workspace_folders = match workspace_folders {
            Some(WorkspaceFolders::WorkspaceFolderList(folders)) => folders,
            Some(WorkspaceFolders::Null) | None => Vec::new(),
        };
        let options: InitializationOptions = initialization_options
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let workspaces = Workspaces::new(
            workspace_folders,
            karva_python_semantic::current_python_version(),
            options.profile,
        )?;
        let connection = connection.initialize_finish(
            id,
            &capabilities,
            crate::SERVER_NAME,
            karva_version::version(),
        )?;

        Ok(Self {
            connection,
            session: Session::new(position_encoding, workspaces),
        })
    }

    /// Handles client messages until the LSP shutdown sequence completes.
    pub fn run(mut self) -> anyhow::Result<()> {
        self.main_loop()
    }
}
