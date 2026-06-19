use camino::{Utf8Path, Utf8PathBuf};
use pyo3::prelude::*;
use ruff_source_file::{OneIndexed, SourceFile, SourceFileBuilder};
use ruff_text_size::{TextRange, TextSize};

/// Parsed representation of a Python traceback from a `PyErr` object.
#[derive(Debug, Clone)]
pub struct Traceback {
    pub lines: Vec<String>,

    pub error_source_file: SourceFile,

    pub location: TextRange,
}

#[derive(Debug, PartialEq)]
struct TracebackLocation {
    file_path: Utf8PathBuf,
    line_number: OneIndexed,
}

impl Traceback {
    pub fn from_error(py: Python, error: &PyErr) -> Option<Self> {
        Self::from_error_with_fallback(py, error, None)
    }

    pub fn from_error_with_source(
        py: Python,
        error: &PyErr,
        source_file: &SourceFile,
    ) -> Option<Self> {
        Self::from_error_with_fallback(py, error, Some(source_file))
    }

    fn from_error_with_fallback(
        py: Python,
        error: &PyErr,
        source_file: Option<&SourceFile>,
    ) -> Option<Self> {
        if let Some(traceback) = error.traceback(py) {
            let traceback_str = match traceback.format() {
                Ok(traceback_str) => traceback_str,
                Err(err) => {
                    tracing::warn!("failed to format Python traceback: {err}");
                    return None;
                }
            };
            if traceback_str.is_empty() {
                return None;
            }
            let lines = filter_traceback(&traceback_str)
                .lines()
                .map(|line| format!(" | {line}"))
                .collect::<Vec<_>>();

            let (error_source_file, location) =
                get_source_file_and_range(&traceback_str, source_file)?;

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
    traceback: &str,
    fallback_source_file: Option<&SourceFile>,
) -> Option<(SourceFile, TextRange)> {
    let traceback_location = get_traceback_location(traceback)?;

    if let Some(source_file) = fallback_source_file
        && source_file_matches_path(source_file, &traceback_location.file_path)
    {
        let text_range =
            calculate_line_range(source_file.source_text(), traceback_location.line_number)?;
        return Some((source_file.clone(), text_range));
    }

    let source_text = std::fs::read_to_string(&traceback_location.file_path).ok()?;

    let source_file =
        SourceFileBuilder::new(traceback_location.file_path.as_str(), source_text.as_str())
            .finish();

    let text_range = calculate_line_range(&source_text, traceback_location.line_number)?;

    Some((source_file, text_range))
}

fn source_file_matches_path(source_file: &SourceFile, traceback_path: &Utf8Path) -> bool {
    let source_path = Utf8Path::new(source_file.name());
    source_path == traceback_path
        || source_path.ends_with(traceback_path)
        || traceback_path.ends_with(source_path)
}

fn get_traceback_location(traceback: &str) -> Option<TracebackLocation> {
    // Find the last line that starts with "File \"" (ignoring leading whitespace)
    for line in traceback.lines().rev() {
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
#[expect(clippy::cast_possible_truncation)]
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
            let leading_whitespace = line_text.len() - line_text.trim_start().len();
            let content_length = line_text.trim().len();

            let range_start = line_start + TextSize::new(leading_whitespace as u32);
            let range_end = range_start + TextSize::new(content_length as u32);

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
    let mut filtered = String::new();

    for (i, line) in traceback.lines().enumerate() {
        if i == 0 && line.contains("Traceback (most recent call last):") {
            continue;
        }
        filtered.push_str(line.strip_prefix("  ").unwrap_or(line));
        filtered.push('\n');
    }
    filtered
        .trim_end_matches('\n')
        .trim_end_matches('^')
        .trim_end()
        .to_string()
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
            insta::assert_snapshot!(filter_traceback(traceback), @r#"
            File "test.py", line 1, in <module>
              raise Exception('Test error')
            Exception: Test error
            "#);
        }

        #[test]
        fn test_filter_traceback_empty() {
            insta::assert_snapshot!(filter_traceback(""), @"");
        }
    }

    mod parse_traceback_line_tests {
        use super::*;

        #[test]
        fn test_parse_traceback_line_valid() {
            let line = r#"  File "test.py", line 10, in <module>"#;
            insta::assert_debug_snapshot!(parse_traceback_line(line), @r#"
            Some(
                TracebackLocation {
                    file_path: "test.py",
                    line_number: OneIndexed(
                        10,
                    ),
                },
            )
            "#);
        }

        #[test]
        fn test_parse_traceback_line_with_path() {
            let line = r#"  File "/path/to/script.py", line 42, in function_name"#;
            insta::assert_debug_snapshot!(parse_traceback_line(line), @r#"
            Some(
                TracebackLocation {
                    file_path: "/path/to/script.py",
                    line_number: OneIndexed(
                        42,
                    ),
                },
            )
            "#);
        }

        #[test]
        fn test_parse_traceback_line_no_file_prefix() {
            insta::assert_debug_snapshot!(parse_traceback_line("Some random line"), @"None");
        }

        #[test]
        fn test_parse_traceback_line_missing_line_number() {
            let line = r#"  File "test.py", in <module>"#;
            insta::assert_debug_snapshot!(parse_traceback_line(line), @"None");
        }

        #[test]
        fn test_parse_traceback_line_malformed_quote() {
            let line = r#"  File "test.py, line 10, in <module>"#;
            insta::assert_debug_snapshot!(parse_traceback_line(line), @"None");
        }

        #[test]
        fn test_parse_traceback_line_large_line_number() {
            let line = r#"  File "test.py", line 99999, in <module>"#;
            insta::assert_debug_snapshot!(parse_traceback_line(line), @r#"
            Some(
                TracebackLocation {
                    file_path: "test.py",
                    line_number: OneIndexed(
                        99999,
                    ),
                },
            )
            "#);
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
            insta::assert_debug_snapshot!(get_traceback_location(traceback), @r#"
            Some(
                TracebackLocation {
                    file_path: "test.py",
                    line_number: OneIndexed(
                        10,
                    ),
                },
            )
            "#);
        }

        #[test]
        fn test_get_traceback_location_multi_frame() {
            let traceback = r#"Traceback (most recent call last):
  File "main.py", line 5, in <module>
    foo()
  File "helper.py", line 15, in foo
    bar()
ValueError: Invalid value"#;
            insta::assert_debug_snapshot!(get_traceback_location(traceback), @r#"
            Some(
                TracebackLocation {
                    file_path: "helper.py",
                    line_number: OneIndexed(
                        15,
                    ),
                },
            )
            "#);
        }

        #[test]
        fn test_get_traceback_location_empty() {
            insta::assert_debug_snapshot!(get_traceback_location(""), @"None");
        }

        #[test]
        fn test_get_traceback_location_no_file_lines() {
            insta::assert_debug_snapshot!(get_traceback_location("Exception: Test error"), @"None");
        }
    }

    mod calculate_line_range_tests {
        use super::*;

        #[test]
        fn test_calculate_line_range_first_line() {
            let source = "line 1\nline 2\nline 3";
            insta::assert_debug_snapshot!(calculate_line_range(source, OneIndexed::new(1).unwrap()), @"
            Some(
                0..6,
            )
            ");
        }

        #[test]
        fn test_calculate_line_range_middle_line() {
            let source = "line 1\nline 2\nline 3";
            insta::assert_debug_snapshot!(calculate_line_range(source, OneIndexed::new(2).unwrap()), @"
            Some(
                7..13,
            )
            ");
        }

        #[test]
        fn test_calculate_line_range_last_line() {
            let source = "line 1\nline 2\nline 3";
            insta::assert_debug_snapshot!(calculate_line_range(source, OneIndexed::new(3).unwrap()), @"
            Some(
                14..20,
            )
            ");
        }

        /// "indented line" is 13 characters, starting at position 11 (after 4 spaces),
        /// and leading/trailing whitespace should be trimmed from the range.
        #[test]
        fn test_calculate_line_range_with_whitespace() {
            let source = "line 1\n    indented line\nline 3";
            insta::assert_debug_snapshot!(calculate_line_range(source, OneIndexed::new(2).unwrap()), @"
            Some(
                11..24,
            )
            ");
        }

        #[test]
        fn test_calculate_line_range_out_of_bounds() {
            let source = "line 1\nline 2";
            insta::assert_debug_snapshot!(calculate_line_range(source, OneIndexed::new(10).unwrap()), @"None");
        }

        #[test]
        fn test_calculate_line_range_single_line() {
            insta::assert_debug_snapshot!(calculate_line_range("single line", OneIndexed::new(1).unwrap()), @"
            Some(
                0..11,
            )
            ");
        }
    }

    mod source_file_range_tests {
        use super::*;

        #[test]
        fn uses_matching_fallback_source_file() {
            let traceback = r#"Traceback (most recent call last):
  File "/tmp/project/tests/test_example.py", line 2, in test_example
    assert False
AssertionError"#;
            let source_file = SourceFileBuilder::new(
                "tests/test_example.py",
                "def test_example():\n    assert False\n",
            )
            .finish();

            let (resolved_source_file, range) =
                get_source_file_and_range(traceback, Some(&source_file)).unwrap();

            assert_eq!(resolved_source_file, source_file);
            assert_eq!(resolved_source_file.slice(range), "assert False");
        }

        #[test]
        fn ignores_fallback_source_file_for_different_path() {
            let traceback = r#"Traceback (most recent call last):
  File "/tmp/project/tests/test_example.py", line 2, in test_example
    assert False
AssertionError"#;
            let source_file = SourceFileBuilder::new(
                "tests/test_other.py",
                "def test_other():\n    assert False\n",
            )
            .finish();

            assert!(get_source_file_and_range(traceback, Some(&source_file)).is_none());
        }
    }
}
