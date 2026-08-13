use lsp_types::{
    LanguageKind, TextDocumentContentChangeEvent, TextDocumentContentChangePartial,
    TextDocumentContentChangeWholeDocument, Uri,
};
use ruff_source_file::LineIndex;

use super::PositionEncoding;
use super::range::range_to_text_range;

/// A document change that violates the client's version contract.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[expect(
    clippy::redundant_pub_crate,
    reason = "document re-exports this error to a private sibling module"
)]
pub(crate) enum DocumentChangeError {
    /// A client attempted to replace newer open-document state with an older version.
    #[error("stale document version: current is {current}, requested {requested}")]
    StaleVersion { current: i32, requested: i32 },
}

/// An open text document, including unsaved editor changes.
#[derive(Debug, Clone)]
pub struct TextDocument {
    uri: Uri,
    contents: String,
    version: i32,
    language_id: LanguageKind,
}

impl TextDocument {
    /// Creates an open document from an LSP `didOpen` notification.
    pub(crate) fn new(uri: Uri, contents: String, version: i32, language_id: LanguageKind) -> Self {
        Self {
            uri,
            contents,
            version,
            language_id,
        }
    }

    /// Returns the URI supplied by the client.
    pub(crate) fn uri(&self) -> &Uri {
        &self.uri
    }

    /// Returns the latest editor contents, including unsaved changes.
    pub(crate) fn contents(&self) -> &str {
        &self.contents
    }

    /// Returns the client-managed document version.
    pub(crate) fn version(&self) -> i32 {
        self.version
    }

    /// Returns the language identifier supplied by the client.
    pub(crate) fn language_id(&self) -> &LanguageKind {
        &self.language_id
    }

    /// Applies changes sequentially using positions from the negotiated encoding.
    pub(crate) fn apply_changes(
        &mut self,
        changes: Vec<TextDocumentContentChangeEvent>,
        new_version: i32,
        encoding: PositionEncoding,
    ) -> Result<(), DocumentChangeError> {
        if new_version < self.version {
            return Err(DocumentChangeError::StaleVersion {
                current: self.version,
                requested: new_version,
            });
        }

        if let [
            TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                TextDocumentContentChangeWholeDocument { text },
            ),
        ] = changes.as_slice()
        {
            self.contents.clone_from(text);
            self.version = new_version;
            return Ok(());
        }

        let mut new_contents = self.contents.clone();
        let mut index = LineIndex::from_source_text(&new_contents);

        for change in changes {
            match change {
                TextDocumentContentChangeEvent::TextDocumentContentChangePartial(
                    TextDocumentContentChangePartial { range, text, .. },
                ) => {
                    let range = range_to_text_range(range, &new_contents, &index, encoding);
                    new_contents
                        .replace_range(usize::from(range.start())..usize::from(range.end()), &text);
                }
                TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                    TextDocumentContentChangeWholeDocument { text },
                ) => new_contents = text,
            }
            index = LineIndex::from_source_text(&new_contents);
        }

        self.contents = new_contents;
        self.version = new_version;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use lsp_types::{
        Position, Range, TextDocumentContentChangeEvent, TextDocumentContentChangePartial,
        TextDocumentContentChangeWholeDocument, Uri,
    };
    use rstest::rstest;

    use super::*;

    fn document(contents: &str) -> TextDocument {
        TextDocument::new(
            Uri::parse("file:///test.py").expect("test URI should be valid"),
            contents.to_owned(),
            1,
            LanguageKind::Python,
        )
    }

    fn partial(line: u32, start: u32, end: u32, text: &str) -> TextDocumentContentChangeEvent {
        TextDocumentContentChangeEvent::TextDocumentContentChangePartial(
            TextDocumentContentChangePartial {
                range: Range::new(Position::new(line, start), Position::new(line, end)),
                text: text.to_owned(),
                ..TextDocumentContentChangePartial::default()
            },
        )
    }

    #[test]
    fn replaces_whole_document() {
        let mut document = document("old");
        document
            .apply_changes(
                vec![
                    TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                        TextDocumentContentChangeWholeDocument {
                            text: "new".to_owned(),
                        },
                    ),
                ],
                2,
                PositionEncoding::UTF16,
            )
            .expect("whole-document change should apply");

        assert_eq!(document.contents(), "new");
        assert_eq!(document.version(), 2);
    }

    #[test]
    fn applies_changes_sequentially() {
        let mut document = document("def test():\n    pas\n");
        document
            .apply_changes(
                vec![
                    partial(1, 7, 7, "s"),
                    partial(1, 7, 8, ""),
                    partial(1, 7, 7, "s"),
                ],
                2,
                PositionEncoding::UTF16,
            )
            .expect("sequential changes should apply");

        assert_eq!(document.contents(), "def test():\n    pass\n");
    }

    #[rstest]
    #[case(PositionEncoding::UTF8, 5)]
    #[case(PositionEncoding::UTF16, 3)]
    #[case(PositionEncoding::UTF32, 2)]
    fn respects_position_encoding(#[case] encoding: PositionEncoding, #[case] end: u32) {
        let mut document = document("a😀b\r\n");
        document
            .apply_changes(vec![partial(0, 1, end, "x")], 2, encoding)
            .expect("encoded range should apply");

        assert_eq!(document.contents(), "axb\r\n");
    }

    #[test]
    fn applies_edits_after_crlf() {
        let mut document = document("first\r\nsecond\r\n");
        document
            .apply_changes(
                vec![partial(1, 0, 6, "changed")],
                2,
                PositionEncoding::UTF16,
            )
            .expect("second-line edit should apply");

        assert_eq!(document.contents(), "first\r\nchanged\r\n");
    }

    #[test]
    fn rejects_stale_version() {
        let mut document = document("current");
        let error = document
            .apply_changes(
                vec![
                    TextDocumentContentChangeEvent::TextDocumentContentChangeWholeDocument(
                        TextDocumentContentChangeWholeDocument {
                            text: "stale".to_owned(),
                        },
                    ),
                ],
                0,
                PositionEncoding::UTF16,
            )
            .expect_err("stale change should fail");

        assert_eq!(
            error,
            DocumentChangeError::StaleVersion {
                current: 1,
                requested: 0,
            }
        );
        assert_eq!(document.contents(), "current");
    }
}
