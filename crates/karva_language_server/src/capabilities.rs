#![expect(
    clippy::redundant_pub_crate,
    reason = "server uses these helpers across private sibling modules"
)]

use lsp_types::{
    ClientCapabilities, CompletionOptions, MarkupKind, RenameOptions, ServerCapabilities,
    TextDocumentSyncKind, TextDocumentSyncOptions, WorkDoneProgressOptions,
    WorkspaceFoldersServerCapabilities,
};

use crate::PositionEncoding;

/// Chooses the most efficient position encoding supported by the client.
pub(super) fn position_encoding(capabilities: &ClientCapabilities) -> PositionEncoding {
    capabilities
        .general
        .as_ref()
        .and_then(|general| general.position_encodings.as_ref())
        .and_then(|encodings| {
            encodings
                .iter()
                .filter_map(|encoding| PositionEncoding::try_from(encoding).ok())
                .max()
        })
        .unwrap_or_default()
}

/// Chooses the client's preferred hover representation.
pub(super) fn hover_markup_kind(capabilities: &ClientCapabilities) -> MarkupKind {
    capabilities
        .text_document
        .as_ref()
        .and_then(|text_document| text_document.hover.as_ref())
        .and_then(|hover| hover.content_format.as_ref())
        .and_then(|formats| formats.first().copied())
        .unwrap_or(MarkupKind::PlainText)
}

/// Returns whether the client accepts secondary diagnostic locations.
pub(super) fn supports_diagnostic_related_information(capabilities: &ClientCapabilities) -> bool {
    capabilities
        .text_document
        .as_ref()
        .and_then(|text_document| text_document.publish_diagnostics.as_ref())
        .and_then(|publish_diagnostics| {
            publish_diagnostics
                .diagnostics_capabilities
                .related_information
        })
        .unwrap_or(false)
}

/// Returns whether the client accepts hierarchical document symbols.
pub(super) fn supports_hierarchical_document_symbols(capabilities: &ClientCapabilities) -> bool {
    capabilities
        .text_document
        .as_ref()
        .and_then(|text_document| text_document.document_symbol.as_ref())
        .and_then(|document_symbol| document_symbol.hierarchical_document_symbol_support)
        .unwrap_or(false)
}

pub(super) fn server_capabilities(position_encoding: PositionEncoding) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(position_encoding.into()),
        text_document_sync: Some(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::Incremental),
                will_save: Some(false),
                will_save_wait_until: Some(false),
                save: None,
            }
            .into(),
        ),
        completion_provider: Some(CompletionOptions::default()),
        definition_provider: Some(true.into()),
        implementation_provider: Some(true.into()),
        hover_provider: Some(true.into()),
        references_provider: Some(true.into()),
        document_highlight_provider: Some(true.into()),
        document_symbol_provider: Some(true.into()),
        rename_provider: Some(
            RenameOptions::new(Some(true), WorkDoneProgressOptions::default()).into(),
        ),
        workspace: Some(lsp_types::WorkspaceOptions {
            workspace_folders: Some(WorkspaceFoldersServerCapabilities {
                supported: Some(true),
                change_notifications: Some(true.into()),
            }),
            ..lsp_types::WorkspaceOptions::default()
        }),
        ..ServerCapabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use lsp_types::{
        ClientCapabilities, DiagnosticsCapabilities, GeneralClientCapabilities,
        HoverClientCapabilities, PositionEncodingKind, PublishDiagnosticsClientCapabilities,
        TextDocumentClientCapabilities,
    };

    use super::*;

    #[test]
    fn defaults_to_utf16() {
        assert_eq!(
            position_encoding(&ClientCapabilities::default()),
            PositionEncoding::UTF16
        );
    }

    #[test]
    fn prefers_utf8() {
        let capabilities = ClientCapabilities {
            general: Some(GeneralClientCapabilities {
                position_encodings: Some(vec![
                    PositionEncodingKind::UTF16,
                    PositionEncodingKind::UTF8,
                ]),
                ..GeneralClientCapabilities::default()
            }),
            ..ClientCapabilities::default()
        };

        assert_eq!(position_encoding(&capabilities), PositionEncoding::UTF8);
    }

    #[test]
    fn resolves_diagnostic_related_information_support() {
        let capabilities = ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                publish_diagnostics: Some(PublishDiagnosticsClientCapabilities {
                    diagnostics_capabilities: DiagnosticsCapabilities {
                        related_information: Some(true),
                        ..DiagnosticsCapabilities::default()
                    },
                    ..PublishDiagnosticsClientCapabilities::default()
                }),
                ..TextDocumentClientCapabilities::default()
            }),
            ..ClientCapabilities::default()
        };

        assert!(supports_diagnostic_related_information(&capabilities));
        assert!(!supports_diagnostic_related_information(
            &ClientCapabilities::default()
        ));
    }

    #[test]
    fn chooses_preferred_hover_markup() {
        let capabilities = ClientCapabilities {
            text_document: Some(TextDocumentClientCapabilities {
                hover: Some(HoverClientCapabilities {
                    content_format: Some(vec![MarkupKind::Markdown, MarkupKind::PlainText]),
                    ..HoverClientCapabilities::default()
                }),
                ..TextDocumentClientCapabilities::default()
            }),
            ..ClientCapabilities::default()
        };

        assert_eq!(hover_markup_kind(&capabilities), MarkupKind::Markdown);
        assert_eq!(
            hover_markup_kind(&ClientCapabilities::default()),
            MarkupKind::PlainText
        );
    }
}
