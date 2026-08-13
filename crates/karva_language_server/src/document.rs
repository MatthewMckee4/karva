//! Open text documents and LSP source-position conversion.

mod range;
mod text_document;

use lsp_types::PositionEncodingKind;

pub use range::{position_to_text_size, text_range_to_range};
pub use text_document::{DocumentChangeError, TextDocument};

/// A supported LSP source-position encoding, ordered by server preference.
#[derive(Default, Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PositionEncoding {
    /// UTF-16 is the encoding every LSP client must support.
    #[default]
    UTF16,

    /// UTF-32 counts Unicode scalar values.
    UTF32,

    /// UTF-8 positions are byte offsets and avoid conversion for Rust strings.
    UTF8,
}

impl From<PositionEncoding> for ruff_source_file::PositionEncoding {
    fn from(value: PositionEncoding) -> Self {
        match value {
            PositionEncoding::UTF8 => Self::Utf8,
            PositionEncoding::UTF16 => Self::Utf16,
            PositionEncoding::UTF32 => Self::Utf32,
        }
    }
}

impl From<PositionEncoding> for PositionEncodingKind {
    fn from(value: PositionEncoding) -> Self {
        match value {
            PositionEncoding::UTF8 => Self::UTF8,
            PositionEncoding::UTF16 => Self::UTF16,
            PositionEncoding::UTF32 => Self::UTF32,
        }
    }
}

impl TryFrom<&PositionEncodingKind> for PositionEncoding {
    type Error = ();

    fn try_from(value: &PositionEncodingKind) -> Result<Self, Self::Error> {
        if value == &PositionEncodingKind::UTF8 {
            Ok(Self::UTF8)
        } else if value == &PositionEncodingKind::UTF32 {
            Ok(Self::UTF32)
        } else if value == &PositionEncodingKind::UTF16 {
            Ok(Self::UTF16)
        } else {
            Err(())
        }
    }
}
