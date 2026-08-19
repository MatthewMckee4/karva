//! Local transport endpoint and lifetime tests.

#[cfg(unix)]
use std::ffi::{OsStr, OsString};
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
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

#[cfg(unix)]
#[test]
fn endpoint_argument_rejects_missing_transport_and_empty_path() {
    assert_eq!(
        ControllerEndpoint::from_argument(OsStr::new("controller.sock")),
        Err("controller endpoint must start with `unix:` or `tcp:`".to_string())
    );
    assert_eq!(
        ControllerEndpoint::from_argument(OsStr::new("unix:")),
        Err("Unix controller endpoint path must not be empty".to_string())
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
