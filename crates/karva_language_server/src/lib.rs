//! Language Server Protocol support for Karva.

use std::sync::Arc;

use anyhow::Context;
use camino::{Utf8Path, Utf8PathBuf};
use karva_metadata::{Options, ProjectMetadata, ProjectOptionsOverrides};
use karva_project::Project;
use ruff_python_ast::PythonVersion;

pub use server::{ConnectionInitializer, Server};

mod capabilities;
mod document;
mod runtime;
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
    /// Previously resolved metadata, valid for this captured source revision.
    cached_projects: Vec<Arc<Project>>,
}

impl PreparedProjectDiscovery {
    /// Reads project configuration and applies initialization overrides.
    fn discover(self) -> Result<Arc<Project>, workspace::WorkspaceError> {
        let project = discover_project(
            &self.path,
            &self.workspace_root,
            self.python_version,
            self.profile.as_deref(),
        )?;
        Ok(self
            .cached_projects
            .into_iter()
            .find(|cached| cached.cwd() == project.cwd())
            .unwrap_or_else(|| Arc::new(project)))
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
pub fn run_server() -> anyhow::Result<()> {
    runtime::initialize_logging()?;
    let (connection, io_threads) = ConnectionInitializer::stdio();
    runtime::with_panic_reporting(connection.sender(), || {
        Server::new(connection)
            .context("failed to initialize language server")?
            .run()
            .context("language-server event loop failed")
    })?;
    // An error must exit promptly, even when the editor keeps stdin open.
    // Only the successful shutdown sequence guarantees that the reader can join.
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| io_threads.join())) {
        Ok(result) => result.context("language-server I/O thread error"),
        Err(payload) => Err(runtime::panic_error(payload.as_ref())),
    }
}
