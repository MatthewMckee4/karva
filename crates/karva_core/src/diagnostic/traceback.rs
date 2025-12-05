use camino::Utf8PathBuf;
use karva_project::System;
use pyo3::prelude::*;
use ruff_source_file::{OneIndexed, SourceFile, SourceFileBuilder};
use ruff_text_size::{TextRange, TextSize};

/// Parsed representation of a Python traceback from a `PyErr` object.
#[derive(Debug, Clone)]
pub struct Traceback {
    #[expect(unused)]
    pub(crate) lines: Vec<String>,

    pub(crate) error_source_file: SourceFile,

    pub(crate) location: TextRange,
}

#[derive(Debug, PartialEq)]
struct TracebackLocation {
    file_path: Utf8PathBuf,
    line_number: OneIndexed,
}

impl Traceback {
    pub(crate) fn from_error(py: Python, system: &dyn System, error: &PyErr) -> Option<Self> {
        if let Some(traceback) = error.traceback(py) {
            let traceback_str = traceback.format().unwrap_or_default();
            if traceback_str.is_empty() {
                return None;
            }
            let lines = filter_traceback(&traceback_str)
                .lines()
                .map(|line| format!(" | {line}"))
                .collect::<Vec<_>>();

            let (error_source_file, location) = get_source_file_and_range(system, &traceback_str)?;

            Some(Self {
                lines,
                error_source_file,
                location,
            })
        } else {
            None
        }
    }
}

fn get_source_file_and_range(
    system: &dyn System,
    traceback: &str,
) -> Option<(SourceFile, TextRange)> {
    let traceback_location = get_traceback_location(traceback)?;

    let source_text = system
        .read_to_string(traceback_location.file_path.as_path())
        .ok()?;

    let source_file =
        SourceFileBuilder::new(traceback_location.file_path.as_str(), source_text.as_str())
            .finish();

    let text_range = calculate_line_range(&source_text, traceback_location.line_number)?;

    Some((source_file, text_range))
}

fn get_traceback_location(traceback: &str) -> Option<TracebackLocation> {
    let lines: Vec<&str> = traceback.lines().collect();

    // Find the last line that starts with "File \"" (ignoring leading whitespace)
    for line in lines.iter().rev() {
        if let Some(location) = parse_traceback_line(line) {
            return Some(location);
        }
    }

    None
}

/// Parse a traceback line like: `  File "/path/to/file.py", line 42, in function_name`
fn parse_traceback_line(line: &str) -> Option<TracebackLocation> {
    let trimmed = line.trim_start();
    let after_file = trimmed.strip_prefix("File \"")?;

    let (filename, rest) = after_file.split_once('"')?;

    let line_str = rest.strip_prefix(", line ")?.split_once(',')?.0;
    let line_number = line_str.parse::<OneIndexed>().ok()?;

    Some(TracebackLocation {
        file_path: Utf8PathBuf::from(filename),
        line_number,
    })
}

/// Calculate the `TextRange` for a specific line in the source text
#[allow(clippy::cast_possible_truncation)]
fn calculate_line_range(source_text: &str, line_number: OneIndexed) -> Option<TextRange> {
    let target_line = line_number.to_zero_indexed();
    let mut current_line = 0;
    let mut line_start = TextSize::default();

    for (idx, ch) in source_text.char_indices() {
        if current_line == target_line {
            // Find the end of this line
            let remaining = &source_text[idx..];
            let line_end = remaining.find('\n').map_or_else(
                || TextSize::new(source_text.len() as u32),
                |newline_pos| TextSize::new((idx + newline_pos) as u32),
            );

            // Trim the line to remove leading/trailing whitespace for the range
            let line_text = &source_text[line_start.to_usize()..line_end.to_usize()];
            let trimmed_start = line_text.len() - line_text.trim_start().len();
            let trimmed_length = line_text.trim().len();

            let range_start = line_start + TextSize::new(trimmed_start as u32);
            let range_end = range_start + TextSize::new(trimmed_length as u32);

            return Some(TextRange::new(range_start, range_end));
        }

        if ch == '\n' {
            current_line += 1;
            line_start = TextSize::new((idx + 1) as u32);
        }
    }

    None
}

// Simplified traceback filtering that removes unnecessary traceback headers
fn filter_traceback(traceback: &str) -> String {
    let lines: Vec<&str> = traceback.lines().collect();
    let mut filtered = String::new();

    for (i, line) in lines.iter().enumerate() {
        if i == 0 && line.contains("Traceback (most recent call last):") {
            continue;
        }
        filtered.push_str(line.strip_prefix("  ").unwrap_or(line));
        filtered.push('\n');
    }
    filtered = filtered.trim_end_matches('\n').to_string();

    filtered = filtered.trim_end_matches('^').to_string();

    filtered.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    mod filter_traceback_tests {
        use super::*;

        #[test]
        fn test_filter_traceback() {
            let traceback = r#"Traceback (most recent call last):
File "test.py", line 1, in <module>
    raise Exception('Test error')
Exception: Test error
"#;
            let filtered = filter_traceback(traceback);
            assert_eq!(
                filtered,
                r#"File "test.py", line 1, in <module>
  raise Exception('Test error')
Exception: Test error"#
            );
        }

        #[test]
        fn test_filter_traceback_empty() {
            let traceback = "";
            let filtered = filter_traceback(traceback);
            assert_eq!(filtered, "");
        }
    }

    mod parse_traceback_line_tests {
        use super::*;

        #[test]
        fn test_parse_traceback_line_valid() {
            let line = r#"  File "test.py", line 10, in <module>"#;
            let location = parse_traceback_line(line);
            let expected_location = Some(TracebackLocation {
                file_path: "test.py".into(),
                line_number: OneIndexed::new(10).unwrap(),
            });
            assert_eq!(location, expected_location);
        }

        #[test]
        fn test_parse_traceback_line_with_path() {
            let line = r#"  File "/path/to/script.py", line 42, in function_name"#;
            let location = parse_traceback_line(line);
            let expected_location = Some(TracebackLocation {
                file_path: "/path/to/script.py".into(),
                line_number: OneIndexed::new(42).unwrap(),
            });
            assert_eq!(location, expected_location);
        }

        #[test]
        fn test_parse_traceback_line_no_file_prefix() {
            let line = "Some random line";
            let location = parse_traceback_line(line);
            assert_eq!(location, None);
        }

        #[test]
        fn test_parse_traceback_line_missing_line_number() {
            let line = r#"  File "test.py", in <module>"#;
            let location = parse_traceback_line(line);
            assert_eq!(location, None);
        }

        #[test]
        fn test_parse_traceback_line_malformed_quote() {
            let line = r#"  File "test.py, line 10, in <module>"#;
            let location = parse_traceback_line(line);
            assert_eq!(location, None);
        }

        #[test]
        fn test_parse_traceback_line_large_line_number() {
            let line = r#"  File "test.py", line 99999, in <module>"#;
            let location = parse_traceback_line(line);
            let expected_location = Some(TracebackLocation {
                file_path: "test.py".into(),
                line_number: OneIndexed::new(99999).unwrap(),
            });
            assert_eq!(location, expected_location);
        }
    }

    mod get_traceback_location_tests {
        use super::*;

        #[test]
        fn test_get_traceback_location_valid() {
            let traceback = r#"Traceback (most recent call last):
  File "test.py", line 10, in <module>
    raise Exception('Test error')
Exception: Test error"#;
            let location = get_traceback_location(traceback);
            let expected_location = Some(TracebackLocation {
                file_path: "test.py".into(),
                line_number: OneIndexed::new(10).unwrap(),
            });
            assert_eq!(location, expected_location);
        }

        #[test]
        fn test_get_traceback_location_multi_frame() {
            let traceback = r#"Traceback (most recent call last):
  File "main.py", line 5, in <module>
    foo()
  File "helper.py", line 15, in foo
    bar()
ValueError: Invalid value"#;
            let location = get_traceback_location(traceback);
            let expected_location = Some(TracebackLocation {
                file_path: "helper.py".into(),
                line_number: OneIndexed::new(15).unwrap(),
            });
            assert_eq!(location, expected_location);
        }

        #[test]
        fn test_get_traceback_location_empty() {
            let traceback = "";
            let location = get_traceback_location(traceback);
            assert_eq!(location, None);
        }

        #[test]
        fn test_get_traceback_location_no_file_lines() {
            let traceback = "Exception: Test error";
            let location = get_traceback_location(traceback);
            assert_eq!(location, None);
        }
    }

    mod calculate_line_range_tests {
        use super::*;

        #[test]
        fn test_calculate_line_range_first_line() {
            let source = "line 1\nline 2\nline 3";
            let range = calculate_line_range(source, OneIndexed::new(1).unwrap());
            assert_eq!(range, Some(TextRange::new(0.into(), 6.into())));
        }

        #[test]
        fn test_calculate_line_range_middle_line() {
            let source = "line 1\nline 2\nline 3";
            let range = calculate_line_range(source, OneIndexed::new(2).unwrap());
            assert_eq!(range, Some(TextRange::new(7.into(), 13.into())));
        }

        #[test]
        fn test_calculate_line_range_last_line() {
            let source = "line 1\nline 2\nline 3";
            let range = calculate_line_range(source, OneIndexed::new(3).unwrap());
            assert_eq!(range, Some(TextRange::new(14.into(), 20.into())));
        }

        #[test]
        fn test_calculate_line_range_with_whitespace() {
            let source = "line 1\n    indented line\nline 3";
            let range = calculate_line_range(source, OneIndexed::new(2).unwrap());
            // Should trim leading/trailing whitespace
            // "indented line" is 13 characters, starting at position 11 (after 4 spaces)
            assert_eq!(range, Some(TextRange::new(11.into(), 24.into())));
        }

        #[test]
        fn test_calculate_line_range_out_of_bounds() {
            let source = "line 1\nline 2";
            let range = calculate_line_range(source, OneIndexed::new(10).unwrap());
            assert_eq!(range, None);
        }

        #[test]
        fn test_calculate_line_range_single_line() {
            let source = "single line";
            let range = calculate_line_range(source, OneIndexed::new(1).unwrap());
            assert_eq!(range, Some(TextRange::new(0.into(), 11.into())));
        }
    }
}
