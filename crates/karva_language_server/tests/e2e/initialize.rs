use anyhow::Context;
use lsp_types::{
    ClientCapabilities, GeneralClientCapabilities, PositionEncodingKind, TextDocumentSync,
    TextDocumentSyncKind,
};

use super::TestServer;

#[test]
fn unknown_request_receives_method_not_found() -> anyhow::Result<()> {
    let mut server = TestServer::new(ClientCapabilities::default());

    let response = server.request_raw("karva/unknown", serde_json::Value::Null);

    let error = response
        .response_result
        .err()
        .context("request should fail")?;
    assert_eq!(error.code, lsp_server::ErrorCode::MethodNotFound as i32);
    assert_eq!(error.message, "unknown request: karva/unknown");
    Ok(())
}

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

#[test]
fn initialization_advertises_document_and_workspace_sync() {
    let server = TestServer::new(ClientCapabilities::default());
    let capabilities = &server.initialization_result().capabilities;
    let Some(TextDocumentSync::Options(sync)) = capabilities.text_document_sync else {
        panic!("expected text document sync options");
    };

    assert_eq!(sync.open_close, Some(true));
    assert_eq!(sync.change, Some(TextDocumentSyncKind::Incremental));
    assert!(capabilities.completion_provider.is_some());
    assert!(capabilities.hover_provider.is_some());
    assert_eq!(
        capabilities
            .workspace
            .as_ref()
            .and_then(|workspace| workspace.workspace_folders.as_ref())
            .and_then(|folders| folders.supported),
        Some(true)
    );
}
