use std::fmt::Write;
use std::io;

use colored::Colorize;
use similar::{Algorithm, ChangeTag, TextDiff};

/// Render a diff between `old` and `new` content into `output`.
///
/// Uses `grouped_ops` for context-aware output with separators between groups,
/// and `iter_inline_changes` for word-level emphasis on changed portions.
fn render_diff(output: &mut String, old: &str, new: &str, width: usize) {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .diff_lines(old, new);
    let ops = diff.grouped_ops(4);

    if ops.is_empty() {
        return;
    }

    let content_width = width.saturating_sub(13);
    let _ = writeln!(output, "────────────┬{:─<content_width$}", "");

    for (group_idx, group) in ops.iter().enumerate() {
        if group_idx > 0 {
            let _ = writeln!(output, "        ┈┈┈┈┼{:┈<content_width$}", "");
        }

        for op in group {
            for change in diff.iter_inline_changes(op) {
                let old_num = format_line_num(change.old_index());
                let new_num = format_line_num(change.new_index());

                let (marker, style) = match change.tag() {
                    ChangeTag::Delete => ("-", Style::Delete),
                    ChangeTag::Insert => ("+", Style::Insert),
                    ChangeTag::Equal => (" ", Style::Equal),
                };

                let mut content = String::new();
                for (emphasized, value) in change.iter_strings_lossy() {
                    if emphasized {
                        match style {
                            Style::Delete => {
                                let _ = write!(content, "{}", value.red().underline());
                            }
                            Style::Insert => {
                                let _ = write!(content, "{}", value.green().underline());
                            }
                            Style::Equal => {
                                let _ = write!(content, "{value}");
                            }
                        }
                    } else {
                        match style {
                            Style::Delete => {
                                let _ = write!(content, "{}", value.red());
                            }
                            Style::Insert => {
                                let _ = write!(content, "{}", value.green());
                            }
                            Style::Equal => {
                                let _ = write!(content, "{}", value.dimmed());
                            }
                        }
                    }
                }

                let colored_marker = match style {
                    Style::Delete => marker.red().to_string(),
                    Style::Insert => marker.green().to_string(),
                    Style::Equal => marker.to_string(),
                };

                let (styled_old, styled_new) = match style {
                    Style::Delete => (old_num.cyan().dimmed().to_string(), new_num.clone()),
                    Style::Insert => (old_num.clone(), new_num.cyan().dimmed().bold().to_string()),
                    Style::Equal => (old_num.dimmed().to_string(), new_num.dimmed().to_string()),
                };

                let _ = write!(
                    output,
                    "{styled_old} {styled_new} │ {colored_marker}{content}",
                );

                if change.missing_newline() {
                    let _ = writeln!(output);
                }
            }
        }
    }

    let _ = writeln!(output, "────────────┴{:─<content_width$}", "");
}

/// Format a diff for use in error messages.
///
/// Uses a fixed total width of 40 characters to match standard border width.
pub fn format_diff(old: &str, new: &str) -> String {
    let mut output = String::new();
    render_diff(&mut output, old, new, 40);
    output
}

/// Write a diff to the given output stream, adapting borders to terminal width.
///
/// Falls back to 80 characters if terminal width cannot be determined.
pub fn print_changeset(out: &mut impl io::Write, old: &str, new: &str) -> io::Result<()> {
    let width = terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80);
    let mut output = String::new();
    render_diff(&mut output, old, new, width);
    write!(out, "{output}")
}

fn format_line_num(num: Option<usize>) -> String {
    match num {
        Some(n) => format!("{:>5}", n + 1),
        None => "     ".to_string(),
    }
}

enum Style {
    Delete,
    Insert,
    Equal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_diff() {
        let result = format_diff("hello\n", "hello\n");
        assert!(
            result.is_empty(),
            "identical content should produce no diff"
        );
    }

    #[test]
    fn test_addition() {
        let result = format_diff("a\n", "a\nb\n");
        assert!(result.contains("+b"));
    }

    #[test]
    fn test_deletion() {
        let result = format_diff("a\nb\n", "a\n");
        assert!(result.contains("-b"));
    }

    #[test]
    fn test_context_separator() {
        let mut lines_old = String::new();
        let mut lines_new = String::new();
        for i in 1..=20 {
            let _ = writeln!(lines_old, "line {i}");
            if i == 1 || i == 20 {
                let _ = writeln!(lines_new, "CHANGED {i}");
            } else {
                let _ = writeln!(lines_new, "line {i}");
            }
        }
        let result = format_diff(&lines_old, &lines_new);
        assert!(
            result.contains("┈┈┈┈┼"),
            "Expected context separator in diff:\n{result}"
        );
    }

    #[test]
    fn test_print_changeset() {
        let mut buf = Vec::new();
        print_changeset(&mut buf, "old\n", "new\n").expect("write should succeed");
        let output = String::from_utf8(buf).expect("valid utf8");
        assert!(output.contains("-old"));
        assert!(output.contains("+new"));
    }
}
