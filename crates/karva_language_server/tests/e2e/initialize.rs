use lsp_types::{ClientCapabilities, GeneralClientCapabilities, PositionEncodingKind};

use super::TestServer;

#[test]
fn initialization_reports_server_info() {
    let server = TestServer::new(ClientCapabilities::default());
    let result = server.initialization_result();

    assert_eq!(
        result.server_info.as_ref().map(|info| info.name.as_str()),
        Some("karva")
    );
    assert_eq!(
        result
            .server_info
            .as_ref()
            .and_then(|info| info.version.as_deref()),
        Some(karva_version::version())
    );
    assert_eq!(
        result.capabilities.position_encoding,
        Some(PositionEncodingKind::UTF16)
    );
}

#[test]
fn initialization_negotiates_utf8() {
    let server = TestServer::new(ClientCapabilities {
        general: Some(GeneralClientCapabilities {
            position_encodings: Some(vec![
                PositionEncodingKind::UTF16,
                PositionEncodingKind::UTF8,
            ]),
            ..GeneralClientCapabilities::default()
        }),
        ..ClientCapabilities::default()
    });

    assert_eq!(
        server
            .initialization_result()
            .capabilities
            .position_encoding,
        Some(PositionEncodingKind::UTF8)
    );
}
