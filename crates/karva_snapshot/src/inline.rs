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

    let mut result: Vec<&str> = lines
        .iter()
        .map(|line| {
            if line.len() >= min_indent {
                &line[min_indent..]
            } else {
                line.trim()
            }
        })
        .collect();

    // Trim trailing empty lines
    while result.last().is_some_and(|l| l.trim().is_empty()) {
        result.pop();
    }

    // Trim leading empty lines
    while result.first().is_some_and(|l| l.trim().is_empty()) {
        result.remove(0);
    }

    result.join("\n")
}

/// Generate a valid Python string literal for the given value.
///
/// - Single-line, no problematic chars: `"value"`
/// - Multi-line: `"""\\\n{indented lines}\n{indent}"""`
pub fn generate_inline_literal(value: &str, indent: usize) -> String {
    let indent_str = " ".repeat(indent);
    let content_indent = " ".repeat(indent + 4);

    if !value.contains('\n') {
        // Single-line: use simple double-quoted string
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        return format!("\"{escaped}\"");
    }

    // Multi-line: use triple-quoted string
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

    // Handle trailing newline in value
    if value.ends_with('\n') {
        // Already have a trailing newline from the last line iteration
    }

    result.push_str(&indent_str);
    result.push_str("\"\"\"");

    result
}

/// Find the inline= argument string literal starting from the given line.
///
/// Text-based scanner that:
/// 1. Finds the line at `line_number` (1-based)
/// 2. Searches forward for `inline=`
/// 3. Parses the Python string literal that follows
/// 4. Returns location info
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

    // Determine the indentation of the call site (first non-empty line from start_line_idx)
    let indent = lines[start_line_idx].len() - lines[start_line_idx].trim_start().len();

    // Search forward from this line for `inline=`
    let search_start = line_byte_offset;
    let search_region = &source[search_start..];

    let inline_pos = search_region.find("inline=")?;
    let abs_inline_pos = search_start + inline_pos;

    // Skip past `inline=`
    let after_eq = abs_inline_pos + "inline=".len();
    if after_eq >= source.len() {
        return None;
    }

    // Parse the string literal that follows
    let (literal_start, literal_end) = parse_string_literal(source, after_eq)?;

    Some(InlineLocation {
        start: literal_start,
        end: literal_end,
        indent,
    })
}

/// Parse a Python string literal at the given byte offset.
/// Returns (start, end) byte offsets including quotes.
fn parse_string_literal(source: &str, offset: usize) -> Option<(usize, usize)> {
    let rest = &source[offset..];
    let rest = rest.trim_start();
    let trimmed_offset = offset + (source[offset..].len() - rest.len());

    // Detect triple or single quote style
    if rest.starts_with("\"\"\"") {
        // Triple double-quoted
        let content_start = trimmed_offset + 3;
        let end = find_triple_quote_end(source, content_start, "\"\"\"")?;
        Some((trimmed_offset, end + 3))
    } else if rest.starts_with("'''") {
        // Triple single-quoted
        let content_start = trimmed_offset + 3;
        let end = find_triple_quote_end(source, content_start, "'''")?;
        Some((trimmed_offset, end + 3))
    } else if rest.starts_with('"') {
        // Single double-quoted
        let content_start = trimmed_offset + 1;
        let end = find_single_quote_end(source, content_start, '"')?;
        Some((trimmed_offset, end + 1))
    } else if rest.starts_with('\'') {
        // Single single-quoted
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
