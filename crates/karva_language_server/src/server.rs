//! Scheduling, I/O, and API endpoints.

use lsp_server::Connection;
use lsp_types::{
    ClientCapabilities, DidChangeWatchedFilesNotification,
    DidChangeWatchedFilesRegistrationOptions, FileSystemWatcher, GlobPattern, InitializeParams,
    Notification as _, Registration, RegistrationParams, RegistrationRequest, WorkspaceFolders,
    WorkspaceFoldersInitializeParams,
};
use ruff_python_ast::PythonVersion;
use serde::Deserialize;

use crate::capabilities::{
    position_encoding, server_capabilities, supports_diagnostic_related_information,
};
use crate::session::Session;
use crate::session::client::Client;
use crate::workspace::Workspaces;

pub use self::connection::ConnectionInitializer;

mod api;
mod connection;
mod main_loop;

/// An initialized Karva language server.
pub struct Server {
    connection: Connection,
    register_config_watchers: bool,
    session: Session,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct InitializationOptions {
    profile: Option<String>,
    python_version: Option<String>,
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
        let register_config_watchers = supports_dynamic_file_watching(&capabilities);
        let supports_diagnostic_related_information =
            supports_diagnostic_related_information(&capabilities);
        let capabilities = server_capabilities(position_encoding);
        let workspace_folders = match workspace_folders {
            Some(WorkspaceFolders::WorkspaceFolderList(folders)) => folders,
            Some(WorkspaceFolders::Null) | None => Vec::new(),
        };
        let options: InitializationOptions = initialization_options
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let python_version = options
            .python_version
            .as_deref()
            .map(str::parse)
            .transpose()?
            .unwrap_or_else(PythonVersion::latest);
        let workspaces = Workspaces::new(workspace_folders, python_version, options.profile)?;
        let connection = connection.initialize_finish(
            id,
            &capabilities,
            crate::SERVER_NAME,
            karva_version::version(),
        )?;

        Ok(Self {
            connection,
            register_config_watchers,
            session: Session::new(
                position_encoding,
                supports_diagnostic_related_information,
                workspaces,
            ),
        })
    }

    /// Handles client messages until the LSP shutdown sequence completes.
    pub fn run(mut self) -> anyhow::Result<()> {
        self.register_config_watchers()?;
        self.main_loop()
    }

    fn register_config_watchers(&mut self) -> anyhow::Result<()> {
        if !self.register_config_watchers {
            return Ok(());
        }

        let id = lsp_server::RequestId::from("karva/register-config-watchers".to_owned());
        self.session
            .request_queue_mut()
            .register_outgoing(id.clone());
        let options = DidChangeWatchedFilesRegistrationOptions {
            watchers: ["**/karva.toml", "**/pyproject.toml"]
                .into_iter()
                .map(|pattern| FileSystemWatcher {
                    glob_pattern: GlobPattern::Pattern(pattern.to_owned()),
                    kind: None,
                })
                .collect(),
        };
        let params = RegistrationParams {
            registrations: vec![Registration {
                id: "karva-config-watchers".to_owned(),
                method: DidChangeWatchedFilesNotification::METHOD
                    .as_str()
                    .to_owned(),
                register_options: Some(serde_json::to_value(options)?),
            }],
        };
        Client::new(self.connection.sender.clone()).send_request::<RegistrationRequest>(id, params)
    }
}

fn supports_dynamic_file_watching(capabilities: &ClientCapabilities) -> bool {
    capabilities
        .workspace
        .as_ref()
        .and_then(|workspace| workspace.did_change_watched_files)
        .and_then(|watched_files| watched_files.dynamic_registration)
        .unwrap_or(false)
}
