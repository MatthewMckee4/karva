use std::io;

/// Location of an inline snapshot string literal in source code.
pub struct InlineLocation {
    /// Byte offset of string literal start (including quotes).
    pub start: usize,
    /// Byte offset of string literal end (including quotes).
    pub end: usize,
    /// Column indentation of the `assert_snapshot` call.
    pub indent: usize,
}

/// Strip common leading whitespace from all non-empty lines and trim trailing whitespace.
///
/// Python evaluates triple-quoted strings with all indentation intact,
/// so we dedent before comparing.
pub fn dedent(raw: &str) -> String {
    let lines: Vec<&str> = raw.lines().collect();

    // Find minimum indentation of non-empty lines
    let min_indent = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);

    let dedented: Vec<&str> = lines
        .iter()
        .map(|line| {
            if line.len() >= min_indent {
                &line[min_indent..]
            } else {
                line.trim()
            }
        })
        .collect();

    // Find the range excluding leading/trailing empty lines
    let first_non_empty = dedented
        .iter()
        .position(|l| !l.trim().is_empty())
        .unwrap_or(0);
    let last_non_empty = dedented
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map_or(0, |i| i + 1);

    if first_non_empty >= last_non_empty {
        return String::new();
    }

    dedented[first_non_empty..last_non_empty].join("\n")
}

/// Generate a valid Python string literal for the given value.
///
/// - Single-line, no problematic chars: `"value"`
/// - Multi-line: `"""\\\n{indented lines}\n{indent}"""`
pub fn generate_inline_literal(value: &str, indent: usize) -> String {
    let indent_str = " ".repeat(indent);
    let content_indent = " ".repeat(indent + 4);

    if !value.contains('\n') {
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        return format!("\"{escaped}\"");
    }

    let mut result = String::from("\"\"\"\\");
    result.push('\n');

    for line in value.lines() {
        if line.is_empty() {
            result.push('\n');
        } else {
            let escaped = line.replace('\\', "\\\\").replace("\"\"\"", "\\\"\\\"\\\"");
            result.push_str(&content_indent);
            result.push_str(&escaped);
            result.push('\n');
        }
    }

    result.push_str(&indent_str);
    result.push_str("\"\"\"");

    result
}

/// Find the `inline=` argument string literal within the `assert_snapshot()` call
/// on or near the given line.
///
/// Searches for `assert_snapshot(` from the given line, then tracks parenthesis
/// depth to find the call boundaries, and only looks for `inline=` within those
/// bounds. This prevents matching `inline=` in unrelated calls further in the file.
pub fn find_inline_argument(source: &str, line_number: u32) -> Option<InlineLocation> {
    let lines: Vec<&str> = source.lines().collect();
    let start_line_idx = (line_number as usize).checked_sub(1)?;

    if start_line_idx >= lines.len() {
        return None;
    }

    // Compute the byte offset of the start of start_line_idx
    let mut line_byte_offset = 0;
    for line in &lines[..start_line_idx] {
        line_byte_offset += line.len() + 1; // +1 for newline
    }

    let indent = lines[start_line_idx].len() - lines[start_line_idx].trim_start().len();

    let search_start = line_byte_offset;
    let (call_pos, call_pattern) = find_snapshot_call(&source[search_start..])?;
    let abs_open_paren = search_start + call_pos + call_pattern.len() - 1;

    // Track paren depth to find the matching close paren
    let call_end = find_matching_close_paren(source, abs_open_paren)?;

    // Search for `inline=` only within the call bounds, skipping string literals
    let abs_inline_pos = find_keyword_in_call(source, abs_open_paren, call_end, "inline=")?;

    let after_eq = abs_inline_pos + "inline=".len();
    if after_eq >= source.len() {
        return None;
    }

    let (literal_start, literal_end) = parse_string_literal(source, after_eq)?;

    Some(InlineLocation {
        start: literal_start,
        end: literal_end,
        indent,
    })
}

const SNAPSHOT_CALL_PATTERNS: &[&str] = &[
    "assert_snapshot(",
    "assert_json_snapshot(",
    "assert_cmd_snapshot(",
];

/// Find the first snapshot assertion call in the given source slice.
///
/// Returns `(position, pattern)` of the earliest match.
fn find_snapshot_call(source: &str) -> Option<(usize, &'static str)> {
    SNAPSHOT_CALL_PATTERNS
        .iter()
        .filter_map(|pattern| source.find(pattern).map(|pos| (pos, *pattern)))
        .min_by_key(|(pos, _)| *pos)
}

/// Find the matching close parenthesis for an open paren at `open_pos`.
///
/// Tracks nesting depth and skips over string literals and comments
/// to avoid matching parens inside them.
fn find_matching_close_paren(source: &str, open_pos: usize) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut depth = 0;
    let mut i = open_pos;

    while i < source.len() {
        match bytes[i] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            b'"' => {
                if i + 2 < source.len() && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
                    i += 3;
                    i = find_triple_quote_end(source, i, "\"\"\"").map(|end| end + 3)?;
                    continue;
                }
                i += 1;
                i = find_single_quote_end(source, i, '"').map(|end| end + 1)?;
                continue;
            }
            b'\'' => {
                if i + 2 < source.len() && bytes[i + 1] == b'\'' && bytes[i + 2] == b'\'' {
                    i += 3;
                    i = find_triple_quote_end(source, i, "'''").map(|end| end + 3)?;
                    continue;
                }
                i += 1;
                i = find_single_quote_end(source, i, '\'').map(|end| end + 1)?;
                continue;
            }
            b'#' => {
                while i < source.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }

    None
}

/// Search for a keyword within a call expression, skipping string literals and comments.
fn find_keyword_in_call(source: &str, start: usize, end: usize, keyword: &str) -> Option<usize> {
    let bytes = source.as_bytes();
    let mut i = start;

    while i < end {
        match bytes[i] {
            b'"' => {
                if i + 2 < end && bytes[i + 1] == b'"' && bytes[i + 2] == b'"' {
                    i = find_triple_quote_end(source, i + 3, "\"\"\"").map(|p| p + 3)?;
                } else {
                    i = find_single_quote_end(source, i + 1, '"').map(|p| p + 1)?;
                }
            }
            b'\'' => {
                if i + 2 < end && bytes[i + 1] == b'\'' && bytes[i + 2] == b'\'' {
                    i = find_triple_quote_end(source, i + 3, "'''").map(|p| p + 3)?;
                } else {
                    i = find_single_quote_end(source, i + 1, '\'').map(|p| p + 1)?;
                }
            }
            b'#' => {
                while i < end && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            _ => {
                if source[i..].starts_with(keyword) {
                    return Some(i);
                }
                i += 1;
            }
        }
    }

    None
}

/// Parse a Python string literal at the given byte offset.
/// Returns (start, end) byte offsets including quotes.
fn parse_string_literal(source: &str, offset: usize) -> Option<(usize, usize)> {
    let rest = &source[offset..];
    let rest = rest.trim_start();
    let trimmed_offset = offset + (source[offset..].len() - rest.len());

    if rest.starts_with("\"\"\"") {
        let content_start = trimmed_offset + 3;
        let end = find_triple_quote_end(source, content_start, "\"\"\"")?;
        Some((trimmed_offset, end + 3))
    } else if rest.starts_with("'''") {
        let content_start = trimmed_offset + 3;
        let end = find_triple_quote_end(source, content_start, "'''")?;
        Some((trimmed_offset, end + 3))
    } else if rest.starts_with('"') {
        let content_start = trimmed_offset + 1;
        let end = find_single_quote_end(source, content_start, '"')?;
        Some((trimmed_offset, end + 1))
    } else if rest.starts_with('\'') {
        let content_start = trimmed_offset + 1;
        let end = find_single_quote_end(source, content_start, '\'')?;
        Some((trimmed_offset, end + 1))
    } else {
        None
    }
}

/// Find the end of a triple-quoted string (position of the closing triple-quote).
fn find_triple_quote_end(source: &str, start: usize, quote: &str) -> Option<usize> {
    let mut i = start;
    let bytes = source.as_bytes();

    while i < source.len() {
        if bytes[i] == b'\\' {
            i += 2; // skip escaped character
            continue;
        }
        if source[i..].starts_with(quote) {
            return Some(i);
        }
        i += 1;
    }

    None
}

/// Find the end of a single-quoted string (position of the closing quote).
fn find_single_quote_end(source: &str, start: usize, quote: char) -> Option<usize> {
    let mut i = start;
    let bytes = source.as_bytes();

    while i < source.len() {
        if bytes[i] == b'\\' {
            i += 2; // skip escaped character
            continue;
        }
        if bytes[i] == quote as u8 {
            return Some(i);
        }
        i += 1;
    }

    None
}

/// Replace a byte range in source text.
pub fn apply_edit(source: &str, start: usize, end: usize, replacement: &str) -> String {
    let mut result = String::with_capacity(source.len() + replacement.len());
    result.push_str(&source[..start]);
    result.push_str(replacement);
    result.push_str(&source[end..]);
    result
}

/// High-level function: read file, find inline argument, generate new literal, write file.
pub fn rewrite_inline_snapshot(
    source_path: &str,
    line_number: u32,
    new_value: &str,
) -> io::Result<()> {
    let source = std::fs::read_to_string(source_path)?;

    let location = find_inline_argument(&source, line_number).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("Could not find inline= argument at {source_path}:{line_number}"),
        )
    })?;

    let new_literal = generate_inline_literal(new_value, location.indent);
    let new_source = apply_edit(&source, location.start, location.end, &new_literal);

    std::fs::write(source_path, new_source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dedent_single_line() {
        assert_eq!(dedent("hello"), "hello");
    }

    #[test]
    fn test_dedent_multi_line() {
        assert_eq!(dedent("    line 1\n    line 2\n"), "line 1\nline 2");
    }

    #[test]
    fn test_dedent_mixed_indent() {
        assert_eq!(
            dedent("    line 1\n        line 2\n    line 3\n"),
            "line 1\n    line 2\nline 3"
        );
    }

    #[test]
    fn test_dedent_empty() {
        assert_eq!(dedent(""), "");
    }

    #[test]
    fn test_dedent_only_whitespace() {
        assert_eq!(dedent("   \n   \n"), "");
    }

    #[test]
    fn test_dedent_with_empty_lines() {
        assert_eq!(dedent("    line 1\n\n    line 2\n"), "line 1\n\nline 2");
    }

    #[test]
    fn test_generate_literal_single_line() {
        assert_eq!(generate_inline_literal("hello", 4), "\"hello\"");
    }

    #[test]
    fn test_generate_literal_with_quotes() {
        assert_eq!(
            generate_inline_literal("say \"hi\"", 4),
            "\"say \\\"hi\\\"\""
        );
    }

    #[test]
    fn test_generate_literal_with_backslash() {
        assert_eq!(
            generate_inline_literal("path\\to\\file", 4),
            "\"path\\\\to\\\\file\""
        );
    }

    #[test]
    fn test_generate_literal_multi_line() {
        let result = generate_inline_literal("line 1\nline 2\n", 4);
        assert_eq!(
            result,
            "\"\"\"\\\n        line 1\n        line 2\n    \"\"\""
        );
    }

    #[test]
    fn test_generate_literal_multi_line_no_trailing_newline() {
        let result = generate_inline_literal("line 1\nline 2", 4);
        assert_eq!(
            result,
            "\"\"\"\\\n        line 1\n        line 2\n    \"\"\""
        );
    }

    #[test]
    fn test_find_inline_simple() {
        let source = "    karva.assert_snapshot('hello', inline=\"\")\n";
        let loc = find_inline_argument(source, 1).expect("should find");
        assert_eq!(&source[loc.start..loc.end], "\"\"");
        assert_eq!(loc.indent, 4);
    }

    #[test]
    fn test_find_inline_with_content() {
        let source = "    karva.assert_snapshot('hello', inline=\"hello world\")\n";
        let loc = find_inline_argument(source, 1).expect("should find");
        assert_eq!(&source[loc.start..loc.end], "\"hello world\"");
    }

    #[test]
    fn test_find_inline_triple_quoted() {
        let source = "    karva.assert_snapshot('hello', inline=\"\"\"hello world\"\"\")\n";
        let loc = find_inline_argument(source, 1).expect("should find");
        assert_eq!(&source[loc.start..loc.end], "\"\"\"hello world\"\"\"");
    }

    #[test]
    fn test_find_inline_single_quoted() {
        let source = "    karva.assert_snapshot('hello', inline='')\n";
        let loc = find_inline_argument(source, 1).expect("should find");
        assert_eq!(&source[loc.start..loc.end], "''");
    }

    #[test]
    fn test_find_inline_multiline_call() {
        let source = "    karva.assert_snapshot(\n        'hello',\n        inline=\"\"\n    )\n";
        let loc = find_inline_argument(source, 1).expect("should find");
        assert_eq!(&source[loc.start..loc.end], "\"\"");
        assert_eq!(loc.indent, 4);
    }

    #[test]
    fn test_find_inline_not_found() {
        let source = "    karva.assert_snapshot('hello')\n";
        assert!(find_inline_argument(source, 1).is_none());
    }

    #[test]
    fn test_find_inline_line_2() {
        let source = "import karva\n    karva.assert_snapshot('hello', inline=\"\")\n";
        let loc = find_inline_argument(source, 2).expect("should find");
        assert_eq!(&source[loc.start..loc.end], "\"\"");
    }

    #[test]
    fn test_find_inline_does_not_match_later_call() {
        let source = "\
    karva.assert_snapshot('hello')
    karva.assert_snapshot('world', inline=\"\")
";
        // Line 1 has no inline=, should NOT match line 2's inline=
        assert!(find_inline_argument(source, 1).is_none());
        // Line 2 should find it
        let loc = find_inline_argument(source, 2).expect("should find on line 2");
        assert_eq!(&source[loc.start..loc.end], "\"\"");
    }

    #[test]
    fn test_find_inline_json_snapshot() {
        let source = "    karva.assert_json_snapshot({'a': 1}, inline=\"\")\n";
        let loc = find_inline_argument(source, 1).expect("should find");
        assert_eq!(&source[loc.start..loc.end], "\"\"");
    }

    #[test]
    fn test_find_inline_skips_string_containing_inline() {
        let source = "    karva.assert_snapshot('inline=bad', inline=\"good\")\n";
        let loc = find_inline_argument(source, 1).expect("should find");
        assert_eq!(&source[loc.start..loc.end], "\"good\"");
    }

    #[test]
    fn test_apply_edit_simple() {
        assert_eq!(apply_edit("hello world", 6, 11, "rust"), "hello rust");
    }

    #[test]
    fn test_apply_edit_empty_to_content() {
        assert_eq!(
            apply_edit("inline=\"\"", 7, 9, "\"hello\""),
            "inline=\"hello\""
        );
    }

    #[test]
    fn test_apply_edit_beginning() {
        assert_eq!(apply_edit("hello", 0, 5, "world"), "world");
    }
}
