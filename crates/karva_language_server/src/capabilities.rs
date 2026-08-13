use lsp_types::{ClientCapabilities, PositionEncodingKind, ServerCapabilities};

/// Chooses the most efficient position encoding supported by the client.
pub fn position_encoding(capabilities: &ClientCapabilities) -> PositionEncodingKind {
    capabilities
        .general
        .as_ref()
        .and_then(|general| general.position_encodings.as_ref())
        .and_then(|encodings| {
            [
                PositionEncodingKind::UTF8,
                PositionEncodingKind::UTF32,
                PositionEncodingKind::UTF16,
            ]
            .into_iter()
            .find(|preferred| encodings.contains(preferred))
        })
        .unwrap_or(PositionEncodingKind::UTF16)
}

pub fn server_capabilities(position_encoding: PositionEncodingKind) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(position_encoding),
        ..ServerCapabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use lsp_types::{ClientCapabilities, GeneralClientCapabilities};

    use super::*;

    #[test]
    fn defaults_to_utf16() {
        assert_eq!(
            position_encoding(&ClientCapabilities::default()),
            PositionEncodingKind::UTF16
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

        assert_eq!(position_encoding(&capabilities), PositionEncodingKind::UTF8);
    }
}
