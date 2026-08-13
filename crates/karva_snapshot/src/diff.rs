use std::borrow::Cow;
use std::fmt::Write;
use std::io;

use colored::Colorize;
use similar::{Algorithm, ChangeTag, TextDiff};

/// Append a trailing newline if the input is non-empty and doesn't already end
/// with one. `diff_lines` keeps each line's terminator as part of the line,
/// so `}\n` and `}` are treated as different lines — normalizing both sides
/// to end with `\n` keeps that comparison stable.
fn ensure_trailing_newline(s: &str) -> Cow<'_, str> {
    if s.is_empty() || s.ends_with('\n') {
        Cow::Borrowed(s)
    } else {
        Cow::Owned(format!("{s}\n"))
    }
}

/// Render a diff between `old` and `new` content into `output`.
///
/// Uses `grouped_ops` for context-aware output with separators between groups,
/// and `iter_inline_changes` for word-level emphasis on changed portions.
fn render_diff(output: &mut String, old: &str, new: &str, width: usize, bordered: bool) {
    let old = ensure_trailing_newline(old);
    let new = ensure_trailing_newline(new);

    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .diff_lines(&old, &new);
    let ops = diff.grouped_ops(4);

    if ops.is_empty() {
        return;
    }

    let max_line = old.lines().count().max(new.lines().count());
    let num_width = if bordered {
        max_line.to_string().len().max(5)
    } else {
        max_line.to_string().len()
    };
    let gutter_width = if bordered {
        2 * num_width + 2
    } else {
        num_width + 1
    };
    let content_width = width.saturating_sub(gutter_width + 1);
    if bordered {
        let _ = writeln!(output, "{:─<gutter_width$}┬{:─<content_width$}", "", "");
    }

    for (group_idx, group) in ops.iter().enumerate() {
        if group_idx > 0 {
            let _ = writeln!(output, "{:┈<gutter_width$}┼{:┈<content_width$}", "", "");
        }

        for op in group {
            for change in diff.iter_inline_changes(op) {
                let (marker, style) = match change.tag() {
                    ChangeTag::Delete => ("-", Style::Delete),
                    ChangeTag::Insert => ("+", Style::Insert),
                    ChangeTag::Equal => (" ", Style::Equal),
                };

                let mut content = String::new();
                for (emphasized, value) in change.iter_strings_lossy() {
                    let _ = write!(content, "{}", style_content(&value, &style, emphasized));
                }

                let colored_marker = style.apply_to_marker(marker);
                if bordered {
                    let old_num = format_line_num(change.old_index(), num_width);
                    let new_num = format_line_num(change.new_index(), num_width);
                    let (styled_old, styled_new) = style.apply_to_line_numbers(old_num, new_num);
                    let _ = write!(
                        output,
                        "{styled_old} {styled_new} │ {colored_marker}{content}",
                    );
                } else {
                    let line_num =
                        format_line_num(change.new_index().or(change.old_index()), num_width);
                    let styled_line_num = style.apply_to_line_number(line_num);
                    let _ = write!(output, "{styled_line_num} │ {colored_marker}{content}");
                }

                if change.missing_newline() {
                    let _ = writeln!(output);
                }
            }
        }
    }

    if bordered {
        let _ = writeln!(output, "{:─<gutter_width$}┴{:─<content_width$}", "", "");
    }
}

/// Format a diff for use in error messages.
///
/// Uses a compact borderless layout with a fixed width of 40 characters.
pub fn format_diff(old: &str, new: &str) -> String {
    let mut output = String::new();
    render_diff(&mut output, old, new, 40, false);
    output
}

/// Write a diff to the given output stream, adapting borders to terminal width.
///
/// Falls back to 80 characters if terminal width cannot be determined.
pub(crate) fn print_changeset(out: &mut impl io::Write, old: &str, new: &str) -> io::Result<()> {
    let width = terminal_size::terminal_size().map_or(80, |(w, _)| w.0 as usize);
    let mut output = String::new();
    render_diff(&mut output, old, new, width, true);
    write!(out, "{output}")
}

fn format_line_num(num: Option<usize>, width: usize) -> String {
    match num {
        Some(n) => format!("{:>width$}", n + 1),
        None => " ".repeat(width),
    }
}

/// Apply color and emphasis to a diff content fragment based on the change style.
fn style_content(value: &str, style: &Style, emphasized: bool) -> String {
    match (style, emphasized) {
        (Style::Delete, true) => value.red().underline().to_string(),
        (Style::Delete, false) => value.red().to_string(),
        (Style::Insert, true) => value.green().underline().to_string(),
        (Style::Insert, false) => value.green().to_string(),
        (Style::Equal, true) => value.to_string(),
        (Style::Equal, false) => value.dimmed().to_string(),
    }
}

enum Style {
    Delete,
    Insert,
    Equal,
}

impl Style {
    /// Color a gutter marker (`-`, `+`, or space) to match the change style.
    fn apply_to_marker(&self, marker: &str) -> String {
        match self {
            Self::Delete => marker.red().to_string(),
            Self::Insert => marker.green().to_string(),
            Self::Equal => marker.to_string(),
        }
    }

    /// Style old/new line number strings to match the change style.
    fn apply_to_line_numbers(&self, old_num: String, new_num: String) -> (String, String) {
        match self {
            Self::Delete => (old_num.cyan().dimmed().to_string(), new_num),
            Self::Insert => (old_num, new_num.cyan().dimmed().bold().to_string()),
            Self::Equal => (old_num.dimmed().to_string(), new_num.dimmed().to_string()),
        }
    }

    /// Style a compact line number to match the change style.
    fn apply_to_line_number(&self, line_num: String) -> String {
        match self {
            Self::Delete => line_num.cyan().dimmed().to_string(),
            Self::Insert => line_num.cyan().dimmed().bold().to_string(),
            Self::Equal => line_num.dimmed().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> insta::Settings {
        let mut settings = insta::Settings::clone_current();
        settings.add_filter(r"\x1b\[[0-9;]*m", "");
        settings.add_filter(r"[-─]{30,}", "[LONG-LINE]");
        settings
    }

    #[test]
    fn no_diff() {
        let result = format_diff("hello\n", "hello\n");
        assert!(
            result.is_empty(),
            "identical content should produce no diff"
        );
    }

    #[test]
    fn addition() {
        settings().bind(|| {
            insta::assert_snapshot!(format_diff("a\n", "a\nb\n"), @"
            1 │  a
            2 │ +b
            ");
        });
    }

    #[test]
    fn deletion() {
        settings().bind(|| {
            insta::assert_snapshot!(format_diff("a\nb\n", "a\n"), @"
            1 │  a
            2 │ -b
            ");
        });
    }

    #[test]
    fn context_separator() {
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
        settings().bind(|| {
            insta::assert_snapshot!(format_diff(&lines_old, &lines_new), @"
             1 │ -line 1
             1 │ +CHANGED 1
             2 │  line 2
             3 │  line 3
             4 │  line 4
             5 │  line 5
            ┈┈┈┼┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈
            16 │  line 16
            17 │  line 17
            18 │  line 18
            19 │  line 19
            20 │ -line 20
            20 │ +CHANGED 20
            ");
        });
    }

    #[test]
    fn large_file_changes_at_boundaries() {
        let mut old = String::new();
        let mut new = String::new();
        for i in 1..=100_000 {
            let _ = writeln!(old, "line {i}");
            if i == 1 || i == 1_000 || i == 10_000 || i == 100_000 {
                let _ = writeln!(new, "CHANGED {i}");
            } else {
                let _ = writeln!(new, "line {i}");
            }
        }
        settings().bind(|| {
            insta::assert_snapshot!(format_diff(&old, &new), @"
                 1 │ -line 1
                 1 │ +CHANGED 1
                 2 │  line 2
                 3 │  line 3
                 4 │  line 4
                 5 │  line 5
            ┈┈┈┈┈┈┈┼┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈
               996 │  line 996
               997 │  line 997
               998 │  line 998
               999 │  line 999
              1000 │ -line 1000
              1000 │ +CHANGED 1000
              1001 │  line 1001
              1002 │  line 1002
              1003 │  line 1003
              1004 │  line 1004
            ┈┈┈┈┈┈┈┼┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈
              9996 │  line 9996
              9997 │  line 9997
              9998 │  line 9998
              9999 │  line 9999
             10000 │ -line 10000
             10000 │ +CHANGED 10000
             10001 │  line 10001
             10002 │  line 10002
             10003 │  line 10003
             10004 │  line 10004
            ┈┈┈┈┈┈┈┼┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈
             99996 │  line 99996
             99997 │  line 99997
             99998 │  line 99998
             99999 │  line 99999
            100000 │ -line 100000
            100000 │ +CHANGED 100000
            ");
        });
    }

    #[test]
    fn trailing_newline_difference_is_ignored() {
        let old = "{\n  \"a\": 1\n}\n";
        let new = "{\n  \"a\": 1\n}";
        assert!(
            format_diff(old, new).is_empty(),
            "trailing-newline-only difference should produce no diff",
        );
    }

    #[test]
    fn one_sided_trailing_newline_in_real_change() {
        let old = "{\n  \"roles\": [\n    \"user\"\n  ]\n}\n";
        let new = "{\n  \"roles\": [\n    \"user\",\n    \"hr\"\n  ]\n}";
        settings().bind(|| {
            insta::assert_snapshot!(format_diff(old, new), @r#"
            1 │  {
            2 │    "roles": [
            3 │ -    "user"
            3 │ +    "user",
            4 │ +    "hr"
            5 │    ]
            6 │  }
            "#);
        });
    }

    #[test]
    fn print_changeset_writes_diff() {
        let mut buf = Vec::new();
        print_changeset(&mut buf, "old\n", "new\n").expect("write should succeed");
        let output = String::from_utf8(buf).expect("valid utf8");
        settings().bind(|| {
            insta::assert_snapshot!(output, @r"
            ────────────┬[LONG-LINE]
                1       │ -old
                      1 │ +new
            ────────────┴[LONG-LINE]
            ");
        });
    }
}
