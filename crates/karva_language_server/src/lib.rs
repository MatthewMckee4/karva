//! Language Server Protocol support for Karva.

use std::io::{self, Write as _};

use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};
use karva_metadata::{Options, ProjectMetadata, ProjectOptionsOverrides};
use karva_project::Project;
use ruff_python_ast::PythonVersion;

pub use server::{ConnectionInitializer, Server};

mod capabilities;
mod document;
mod server;
mod session;
mod workspace;

pub use document::{PositionEncoding, TextDocument};

const SERVER_NAME: &str = "karva";

/// Owned inputs for resolving one document's Karva project off the event loop.
#[derive(Clone, Debug)]
struct PreparedProjectDiscovery {
    path: Utf8PathBuf,
    workspace_root: Utf8PathBuf,
    python_version: PythonVersion,
    profile: Option<String>,
}

impl PreparedProjectDiscovery {
    /// Reads project configuration and applies initialization overrides.
    fn discover(self) -> Result<Project, workspace::WorkspaceError> {
        discover_project(
            &self.path,
            &self.workspace_root,
            self.python_version,
            self.profile.as_deref(),
        )
    }
}

fn discover_project(
    path: &Utf8Path,
    workspace_root: &Utf8Path,
    python_version: PythonVersion,
    profile: Option<&str>,
) -> Result<Project, workspace::WorkspaceError> {
    let directory = path
        .parent()
        .ok_or_else(|| workspace::WorkspaceError::MissingParent(path.to_path_buf()))?;
    let mut metadata = ProjectMetadata::discover(directory, python_version)?;
    if metadata.root() == directory
        && !directory.join("karva.toml").exists()
        && !directory.join("pyproject.toml").exists()
    {
        let fallback_root = directory
            .ancestors()
            .take_while(|ancestor| ancestor.starts_with(workspace_root))
            .find(|ancestor| ancestor.join(".git").exists())
            .unwrap_or(workspace_root);
        metadata = ProjectMetadata::discover(fallback_root, python_version)?;
    }
    metadata.apply_overrides(
        &ProjectOptionsOverrides::new(None, Options::default())
            .with_profile(profile.map(str::to_owned)),
    )?;
    Ok(Project::from_metadata(metadata))
}

/// Runs the Karva language server over standard input and output.
pub fn run() -> anyhow::Result<()> {
    match std::env::args().nth(1).as_deref() {
        None => run_server(),
        Some("--version" | "-V") => {
            write_stdout(&format!("{SERVER_NAME} {}", karva_version::version()))
        }
        Some("--help" | "-h") => write_stdout(
            "Karva language server\n\nUsage: karva-language-server [OPTIONS]\n\nOptions:\n  -V, --version  Print the server version\n  -h, --help     Print this help message",
        ),
        Some(argument) => anyhow::bail!(
            "unexpected argument `{argument}`; the language server communicates over stdio"
        ),
    }
}

fn write_stdout(output: &str) -> anyhow::Result<()> {
    writeln!(io::stdout().lock(), "{output}").context("failed to write command output")
}

fn run_server() -> anyhow::Result<()> {
    let (connection, io_threads) = ConnectionInitializer::stdio();
    let server_result = Server::new(connection)
        .context("failed to initialize language server")?
        .run();
    let io_result = io_threads.join();

    match (server_result, io_result) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(server), Err(io)) => Err(server).context(format!("I/O thread error: {io}")),
        (Err(server), _) => Err(server),
        (_, Err(io)) => Err(io).context("I/O thread error"),
    }
}
