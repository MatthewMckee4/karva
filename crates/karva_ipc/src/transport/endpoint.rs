//! Worker-facing local endpoint encoding.

use std::ffi::{OsStr, OsString};
use std::fmt;
use std::net::SocketAddr;
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(unix)]
use std::path::PathBuf;

/// Local endpoint passed from a controller to its worker processes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ControllerEndpoint {
    /// Loopback TCP endpoint used on Windows and as a portable fallback.
    Tcp(SocketAddr),

    /// Filesystem-backed Unix-domain endpoint used on Unix platforms.
    #[cfg(unix)]
    Unix(PathBuf),
}

impl fmt::Display for ControllerEndpoint {
    /// Renders a diagnostic label; subprocess arguments use the lossless encoding.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tcp(address) => address.fmt(formatter),
            #[cfg(unix)]
            Self::Unix(path) => write!(formatter, "unix:{}", path.display()),
        }
    }
}

impl ControllerEndpoint {
    /// Encodes this endpoint without losing non-Unicode Unix path bytes.
    pub fn to_argument(&self) -> OsString {
        match self {
            Self::Tcp(address) => format!("tcp:{address}").into(),
            #[cfg(unix)]
            Self::Unix(path) => {
                let mut argument = b"unix:".to_vec();
                argument.extend_from_slice(path.as_os_str().as_bytes());
                OsString::from_vec(argument)
            }
        }
    }

    /// Decodes the private worker argument used for this platform's transport.
    pub fn from_argument(value: &OsStr) -> Result<Self, String> {
        #[cfg(unix)]
        {
            let value = value.as_bytes();
            if let Some(path) = value.strip_prefix(b"unix:") {
                if path.is_empty() {
                    return Err("Unix controller endpoint path must not be empty".to_string());
                }
                return Ok(Self::Unix(PathBuf::from(OsString::from_vec(path.to_vec()))));
            }
            let Some(address) = value.strip_prefix(b"tcp:") else {
                return Err("controller endpoint must start with `unix:` or `tcp:`".to_string());
            };
            let address = std::str::from_utf8(address)
                .map_err(|_| "TCP controller endpoint must be valid Unicode".to_string())?;
            parse_tcp_endpoint(address)
        }

        #[cfg(not(unix))]
        {
            let value = value
                .to_str()
                .ok_or_else(|| "TCP controller endpoint must be valid Unicode".to_string())?;
            let Some(address) = value.strip_prefix("tcp:") else {
                return Err("controller endpoint must start with `tcp:`".to_string());
            };
            parse_tcp_endpoint(address)
        }
    }
}

fn parse_tcp_endpoint(address: &str) -> Result<ControllerEndpoint, String> {
    if address.is_empty() {
        return Err("TCP controller endpoint must not be empty".to_string());
    }
    address
        .parse()
        .map(ControllerEndpoint::Tcp)
        .map_err(|error| format!("invalid TCP controller endpoint `{address}`: {error}"))
}
