//! Controller-side local listener ownership.

use std::io;
use std::net::{Ipv4Addr, TcpListener};
#[cfg(unix)]
use std::os::unix::net::UnixListener;
#[cfg(unix)]
use std::path::PathBuf;

use anyhow::{Context, Result};
#[cfg(unix)]
use tempfile::TempDir;

use super::{ControllerEndpoint, ControllerStream};

/// Controller-side local listener selected for one test run.
pub enum ControllerListener {
    /// Loopback TCP listener.
    Tcp {
        /// Socket accepting worker connections.
        listener: TcpListener,

        /// Bound address passed to workers.
        endpoint: ControllerEndpoint,
    },

    /// Unix-domain listener and its path, removed when the listener drops.
    #[cfg(unix)]
    Unix {
        /// Socket accepting worker connections.
        listener: UnixListener,

        /// Private directory keeping the socket path short and collision-free.
        _directory: TempDir,

        /// Socket path unlinked before the private directory is released.
        path: PathBuf,
    },
}

impl ControllerListener {
    /// Binds the fastest local endpoint available on this platform.
    pub fn bind() -> Result<Self> {
        #[cfg(unix)]
        {
            Self::bind_unix().or_else(|unix_error| {
                Self::bind_tcp().with_context(|| {
                    format!(
                        "failed to bind a Unix controller socket ({unix_error:#}) or its loopback TCP fallback"
                    )
                })
            })
        }

        #[cfg(not(unix))]
        {
            Self::bind_tcp()
        }
    }

    #[cfg(unix)]
    fn bind_unix() -> Result<Self> {
        let directory =
            TempDir::new().context("failed to create Karva controller socket directory")?;
        let path = directory.path().join("controller.sock");
        let listener = UnixListener::bind(&path).with_context(|| {
            format!("failed to bind Unix controller socket `{}`", path.display())
        })?;
        Ok(Self::Unix {
            listener,
            _directory: directory,
            path,
        })
    }

    pub fn bind_tcp() -> Result<Self> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .context("failed to bind Karva controller listener")?;
        let endpoint = listener
            .local_addr()
            .map(ControllerEndpoint::Tcp)
            .context("failed to read Karva controller listener address")?;
        Ok(Self::Tcp { listener, endpoint })
    }

    /// Configures nonblocking acceptance for the controller event loop.
    pub fn set_nonblocking(&self, nonblocking: bool) -> Result<()> {
        match self {
            Self::Tcp { listener, .. } => listener
                .set_nonblocking(nonblocking)
                .context("failed to configure Karva controller listener"),
            #[cfg(unix)]
            Self::Unix { listener, .. } => listener
                .set_nonblocking(nonblocking)
                .context("failed to configure Unix controller socket"),
        }
    }

    /// Returns the worker-facing endpoint.
    pub fn endpoint(&self) -> ControllerEndpoint {
        match self {
            Self::Tcp { endpoint, .. } => endpoint.clone(),
            #[cfg(unix)]
            Self::Unix { path, .. } => ControllerEndpoint::Unix(path.clone()),
        }
    }

    /// Accepts one queued worker connection.
    pub fn accept(&self) -> io::Result<ControllerStream> {
        match self {
            Self::Tcp { listener, .. } => listener
                .accept()
                .map(|(stream, _)| ControllerStream::Tcp(stream)),
            #[cfg(unix)]
            Self::Unix { listener, .. } => listener
                .accept()
                .map(|(stream, _)| ControllerStream::Unix(stream)),
        }
    }
}

impl Drop for ControllerListener {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Self::Unix { path, .. } = self {
            let _ = std::fs::remove_file(path);
        }
        #[cfg(not(unix))]
        let _ = self;
    }
}
