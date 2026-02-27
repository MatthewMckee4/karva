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

    let max_line = old.lines().count().max(new.lines().count());
    let num_width = max_line.to_string().len().max(5);
    let gutter_width = 2 * num_width + 2;
    let content_width = width.saturating_sub(gutter_width + 1);
    let separator_pad = gutter_width.saturating_sub(4);
    let _ = writeln!(output, "{:─<gutter_width$}┬{:─<content_width$}", "", "");

    for (group_idx, group) in ops.iter().enumerate() {
        if group_idx > 0 {
            let _ = writeln!(output, "{:separator_pad$}┈┈┈┈┼{:┈<content_width$}", "", "");
        }

        for op in group {
            for change in diff.iter_inline_changes(op) {
                let old_num = format_line_num(change.old_index(), num_width);
                let new_num = format_line_num(change.new_index(), num_width);

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

    let _ = writeln!(output, "{:─<gutter_width$}┴{:─<content_width$}", "", "");
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

fn format_line_num(num: Option<usize>, width: usize) -> String {
    match num {
        Some(n) => format!("{:>width$}", n + 1),
        None => " ".repeat(width),
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
            insta::assert_snapshot!(format_diff("a\n", "a\nb\n"), @r"
            ────────────┬───────────────────────────
                1     1 │  a
                      2 │ +b
            ────────────┴───────────────────────────
            ");
        });
    }

    #[test]
    fn deletion() {
        settings().bind(|| {
            insta::assert_snapshot!(format_diff("a\nb\n", "a\n"), @r"
            ────────────┬───────────────────────────
                1     1 │  a
                2       │ -b
            ────────────┴───────────────────────────
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
            insta::assert_snapshot!(format_diff(&lines_old, &lines_new), @r"
            ────────────┬───────────────────────────
                1       │ -line 1
                      1 │ +CHANGED 1
                2     2 │  line 2
                3     3 │  line 3
                4     4 │  line 4
                5     5 │  line 5
                    ┈┈┈┈┼┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈
               16    16 │  line 16
               17    17 │  line 17
               18    18 │  line 18
               19    19 │  line 19
               20       │ -line 20
                     20 │ +CHANGED 20
            ────────────┴───────────────────────────
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
            insta::assert_snapshot!(format_diff(&old, &new), @r"
            ──────────────┬─────────────────────────
                 1        │ -line 1
                        1 │ +CHANGED 1
                 2      2 │  line 2
                 3      3 │  line 3
                 4      4 │  line 4
                 5      5 │  line 5
                      ┈┈┈┈┼┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈
               996    996 │  line 996
               997    997 │  line 997
               998    998 │  line 998
               999    999 │  line 999
              1000        │ -line 1000
                     1000 │ +CHANGED 1000
              1001   1001 │  line 1001
              1002   1002 │  line 1002
              1003   1003 │  line 1003
              1004   1004 │  line 1004
                      ┈┈┈┈┼┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈
              9996   9996 │  line 9996
              9997   9997 │  line 9997
              9998   9998 │  line 9998
              9999   9999 │  line 9999
             10000        │ -line 10000
                    10000 │ +CHANGED 10000
             10001  10001 │  line 10001
             10002  10002 │  line 10002
             10003  10003 │  line 10003
             10004  10004 │  line 10004
                      ┈┈┈┈┼┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈┈
             99996  99996 │  line 99996
             99997  99997 │  line 99997
             99998  99998 │  line 99998
             99999  99999 │  line 99999
            100000        │ -line 100000
                   100000 │ +CHANGED 100000
            ──────────────┴─────────────────────────
            ");
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
