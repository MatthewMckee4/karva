use lsp_types::{Position, Range};
use ruff_source_file::{LineIndex, OneIndexed, SourceLocation};
use ruff_text_size::{TextRange, TextSize};

use super::PositionEncoding;

fn position_to_text_size(
    position: Position,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> TextSize {
    index.offset(
        SourceLocation {
            line: OneIndexed::from_zero_indexed(position.line as usize),
            character_offset: OneIndexed::from_zero_indexed(position.character as usize),
        },
        text,
        encoding.into(),
    )
}

pub(super) fn range_to_text_range(
    range: Range,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> TextRange {
    TextRange::new(
        position_to_text_size(range.start, text, index, encoding),
        position_to_text_size(range.end, text, index, encoding),
    )
}
