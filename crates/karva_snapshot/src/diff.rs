use std::fmt::Write;

use colored::Colorize;
use similar::{Algorithm, ChangeTag, TextDiff};

/// Format a line number for display in the diff gutter.
///
/// Numbers are right-aligned in a 4-char field; blanks are shown for absent lines.
fn format_line_num(num: Option<usize>) -> String {
    match num {
        Some(n) => format!("{:>4}", n + 1),
        None => "    ".to_string(),
    }
}

/// Generate an insta-style diff between old and new content.
///
/// Produces output with box-drawing characters, dual line numbers, and colored changes.
pub fn format_diff(old: &str, new: &str) -> String {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .diff_lines(old, new);

    let mut output = String::new();

    let _ = writeln!(output, "{}", "-old snapshot".red());
    let _ = writeln!(output, "{}", "+new results".green());

    let _ = writeln!(output, "────────────┬───────────────────────────");

    for change in diff.iter_all_changes() {
        let old_num = format_line_num(change.old_index());
        let new_num = format_line_num(change.new_index());
        let text = change.as_str().unwrap_or("");
        let text = text.strip_suffix('\n').unwrap_or(text);

        let colored_content = match change.tag() {
            ChangeTag::Delete => format!("-{text}").red().to_string(),
            ChangeTag::Insert => format!("+{text}").green().to_string(),
            ChangeTag::Equal => format!(" {text}"),
        };

        let _ = writeln!(
            output,
            "{} {} {} {colored_content}",
            old_num.dimmed(),
            new_num.dimmed(),
            "│".dimmed(),
        );
    }

    let _ = writeln!(output, "────────────┴───────────────────────────");

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_diff() {
        let result = format_diff("hello\n", "hello\n");
        assert!(result.contains("hello"));
        assert!(result.contains("│"));
        assert!(result.contains("────────────┬"));
        assert!(result.contains("────────────┴"));
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
    fn test_headers() {
        let result = format_diff("old\n", "new\n");
        assert!(result.contains("-old snapshot"));
        assert!(result.contains("+new results"));
    }
}
