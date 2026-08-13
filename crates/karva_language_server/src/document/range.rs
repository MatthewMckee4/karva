use lsp_types::{Position, Range};
use ruff_source_file::{LineIndex, OneIndexed, SourceLocation};
use ruff_text_size::{TextRange, TextSize};

use super::PositionEncoding;

#[expect(
    clippy::redundant_pub_crate,
    reason = "server endpoints consume position conversion through the document boundary"
)]
pub(crate) fn position_to_text_size(
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

fn source_location_to_position(location: SourceLocation) -> Option<Position> {
    Some(Position::new(
        u32::try_from(location.line.to_zero_indexed()).ok()?,
        u32::try_from(location.character_offset.to_zero_indexed()).ok()?,
    ))
}

/// Converts a UTF-8 byte range to the position encoding negotiated with the client.
#[expect(
    clippy::redundant_pub_crate,
    reason = "server endpoints consume range conversion through the document boundary"
)]
pub(crate) fn text_range_to_range(
    range: TextRange,
    text: &str,
    index: &LineIndex,
    encoding: PositionEncoding,
) -> Option<Range> {
    Some(Range::new(
        source_location_to_position(index.source_location(range.start(), text, encoding.into()))?,
        source_location_to_position(index.source_location(range.end(), text, encoding.into()))?,
    ))
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
