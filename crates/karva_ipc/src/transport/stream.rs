//! Connected local stream operations shared by controller and worker.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
#[cfg(unix)]
use std::os::unix::net::UnixStream;

use anyhow::{Context, Result};

use super::ControllerEndpoint;

/// Connected local stream shared by the worker and controller reader.
pub enum ControllerStream {
    /// Loopback TCP stream.
    Tcp(TcpStream),

    /// Unix-domain stream.
    #[cfg(unix)]
    Unix(UnixStream),
}

impl ControllerStream {
    /// Connects to a controller endpoint.
    pub fn connect(endpoint: &ControllerEndpoint) -> Result<Self> {
        match endpoint {
            ControllerEndpoint::Tcp(address) => TcpStream::connect(address)
                .map(Self::Tcp)
                .with_context(|| format!("failed to connect to Karva controller at {address}")),
            #[cfg(unix)]
            ControllerEndpoint::Unix(path) => {
                UnixStream::connect(path).map(Self::Unix).with_context(|| {
                    format!(
                        "failed to connect to Unix controller socket `{}`",
                        path.display()
                    )
                })
            }
        }
    }

    /// Disables Nagle buffering for low-latency crash checkpoints on TCP.
    pub fn set_nodelay(&self, enabled: bool) -> Result<()> {
        match self {
            Self::Tcp(stream) => stream
                .set_nodelay(enabled)
                .context("failed to configure Karva controller connection"),
            #[cfg(unix)]
            Self::Unix(_) => Ok(()),
        }
    }

    /// Clones the stream for independent reader and shutdown handles.
    pub fn try_clone(&self) -> Result<Self> {
        match self {
            Self::Tcp(stream) => stream
                .try_clone()
                .map(Self::Tcp)
                .context("failed to clone Karva controller connection"),
            #[cfg(unix)]
            Self::Unix(stream) => stream
                .try_clone()
                .map(Self::Unix)
                .context("failed to clone Unix controller connection"),
        }
    }

    /// Configures blocking mode for a controller reader.
    pub fn set_nonblocking(&self, nonblocking: bool) -> Result<()> {
        match self {
            Self::Tcp(stream) => stream
                .set_nonblocking(nonblocking)
                .context("failed to configure Karva worker connection"),
            #[cfg(unix)]
            Self::Unix(stream) => stream
                .set_nonblocking(nonblocking)
                .context("failed to configure Unix worker connection"),
        }
    }

    /// Interrupts a reader or closes a worker connection.
    pub fn shutdown(&self) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.shutdown(Shutdown::Both),
            #[cfg(unix)]
            Self::Unix(stream) => stream.shutdown(Shutdown::Both),
        }
    }
}

impl Read for ControllerStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(buffer),
            #[cfg(unix)]
            Self::Unix(stream) => stream.read(buffer),
        }
    }
}

impl Write for ControllerStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(buffer),
            #[cfg(unix)]
            Self::Unix(stream) => stream.write(buffer),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            #[cfg(unix)]
            Self::Unix(stream) => stream.flush(),
        }
    }
}
