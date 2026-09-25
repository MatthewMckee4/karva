//! Local transport endpoint and lifetime tests.

use std::ffi::OsStr;
#[cfg(unix)]
use std::ffi::OsString;
#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(unix)]
use std::path::PathBuf;

use super::{ControllerEndpoint, ControllerListener};

#[test]
fn endpoint_argument_roundtrips() {
    let endpoint = ControllerListener::bind()
        .expect("bind controller listener")
        .endpoint();
    let encoded = endpoint.to_argument();
    assert_eq!(ControllerEndpoint::from_argument(&encoded), Ok(endpoint));
}

#[cfg(unix)]
#[test]
fn non_unicode_unix_endpoint_argument_roundtrips() {
    let endpoint = ControllerEndpoint::Unix(PathBuf::from(OsString::from_vec(vec![0xff])));
    let encoded = endpoint.to_argument();

    assert_eq!(ControllerEndpoint::from_argument(&encoded), Ok(endpoint));
}

#[test]
fn tcp_fallback_endpoint_argument_roundtrips() {
    let listener = ControllerListener::bind_tcp().expect("bind TCP controller listener");
    let endpoint = listener.endpoint();
    let encoded = endpoint.to_argument();

    assert_eq!(ControllerEndpoint::from_argument(&encoded), Ok(endpoint));
}

#[test]
fn endpoint_argument_rejects_invalid_transport_values() {
    let missing_transport_error = if cfg!(unix) {
        "controller endpoint must start with `unix:` or `tcp:`"
    } else {
        "controller endpoint must start with `tcp:`"
    };
    assert_eq!(
        ControllerEndpoint::from_argument(OsStr::new("controller.sock")),
        Err(missing_transport_error.to_string())
    );
    assert_eq!(
        ControllerEndpoint::from_argument(OsStr::new("tcp:")),
        Err("TCP controller endpoint must not be empty".to_string())
    );
    assert!(matches!(
        ControllerEndpoint::from_argument(OsStr::new("tcp:not-an-address")),
        Err(error) if error.starts_with("invalid TCP controller endpoint `not-an-address`:")
    ));
}

#[cfg(unix)]
#[test]
fn unix_endpoint_argument_rejects_empty_path_and_invalid_tcp_encoding() {
    assert_eq!(
        ControllerEndpoint::from_argument(OsStr::new("unix:")),
        Err("Unix controller endpoint path must not be empty".to_string())
    );
    assert_eq!(
        ControllerEndpoint::from_argument(OsStr::from_bytes(b"tcp:\xff")),
        Err("TCP controller endpoint must be valid Unicode".to_string())
    );
}

#[cfg(unix)]
#[test]
fn unix_endpoint_is_removed_when_listener_drops() {
    let listener = ControllerListener::bind().expect("bind controller listener");
    let ControllerEndpoint::Unix(path) = listener.endpoint() else {
        panic!("Unix platforms must use Unix controller endpoints");
    };
    assert!(path.exists());
    drop(listener);
    assert!(!path.exists());
}
