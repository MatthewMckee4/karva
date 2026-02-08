use std::fmt::Write;

use colored::Colorize;
use similar::{Algorithm, ChangeTag, TextDiff};

/// Generate a unified diff between old and new content.
///
/// Returns a formatted string with `+` for additions (green) and `-` for deletions (red).
pub fn format_diff(old: &str, new: &str) -> String {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .diff_lines(old, new);

    let mut output = String::new();
    for change in diff.iter_all_changes() {
        let line = match change.tag() {
            ChangeTag::Delete => format!("-{change}").red().to_string(),
            ChangeTag::Insert => format!("+{change}").green().to_string(),
            ChangeTag::Equal => format!(" {change}"),
        };
        let _ = write!(output, "{line}");
        if !change.as_str().unwrap_or("").ends_with('\n') {
            output.push('\n');
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_diff() {
        let result = format_diff("hello\n", "hello\n");
        assert_eq!(result, " hello\n");
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
}
