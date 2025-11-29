use camino::Utf8PathBuf;
use pyo3::prelude::*;
use ruff_source_file::OneIndexed;

#[derive(Debug, Clone)]
pub struct Traceback {
    pub(crate) lines: Vec<String>,

    pub(crate) location: Option<Location>,
}

impl Traceback {
    pub(crate) fn new(py: Python<'_>, cwd: &Utf8PathBuf, error: &PyErr) -> Self {
        if let Some(traceback) = error.traceback(py) {
            let traceback_str = traceback.format().unwrap_or_default();
            if traceback_str.is_empty() {
                return Self {
                    lines: vec![],
                    location: None,
                };
            }
            let lines = filter_traceback(&traceback_str)
                .lines()
                .map(|line| format!(" | {line}"))
                .collect::<Vec<_>>();

            Self {
                lines,
                location: get_location(cwd, &traceback_str),
            }
        } else {
            Self {
                lines: vec![],
                location: None,
            }
        }
    }
}

fn get_location(cwd: &Utf8PathBuf, traceback: &str) -> Option<Location> {
    let lines: Vec<&str> = traceback.lines().collect();

    // Find the last line that starts with "File \"" (ignoring leading whitespace)
    for line in lines.iter().rev() {
        if let Some(location) = parse_traceback_line(cwd, line) {
            return Some(location);
        }
    }

    None
}

/// Wow this is ugly, but it works!
fn parse_traceback_line(cwd: &Utf8PathBuf, line: &str) -> Option<Location> {
    let trimmed = line.trim_start();
    let after_file = trimmed.strip_prefix("File \"")?;

    let (filename, rest) = after_file.split_once('"')?;
    let file = Utf8PathBuf::from(filename);
    let relative_path = file.strip_prefix(cwd).unwrap_or(&file).to_path_buf();

    let line_str = rest.strip_prefix(", line ")?.split_once(',')?.0;
    let line_number = line_str.parse::<OneIndexed>().ok()?;

    Some(Location::new(relative_path, line_number))
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

    mod get_location_tests {
        use ruff_source_file::OneIndexed;

        use super::*;

        #[test]
        fn test_get_location_valid_traceback() {
            let traceback = r#"Traceback (most recent call last):
  File "test.py", line 10, in <module>
    raise Exception('Test error')
Exception: Test error"#;
            let location = get_location(&Utf8PathBuf::new(), traceback);
            let expected_location = Some(Location::new(
                "test.py".into(),
                OneIndexed::new(10).unwrap(),
            ));
            assert_eq!(location, expected_location);
        }

        #[test]
        fn test_get_location_with_path() {
            let traceback = r#"Traceback (most recent call last):
  File "/path/to/script.py", line 42, in function_name
    some_code()
RuntimeError: Something went wrong"#;
            let location = get_location(&Utf8PathBuf::new(), traceback);
            let expected_location = Some(Location::new(
                "/path/to/script.py".into(),
                OneIndexed::new(42).unwrap(),
            ));
            assert_eq!(location, expected_location);
        }

        #[test]
        fn test_get_location_multi_frame() {
            let traceback = r#"Traceback (most recent call last):
  File "main.py", line 5, in <module>
    foo()
  File "helper.py", line 15, in foo
    bar()
ValueError: Invalid value"#;
            let location = get_location(&Utf8PathBuf::new(), traceback);
            let expected_location = Some(Location::new(
                "helper.py".into(),
                OneIndexed::new(15).unwrap(),
            ));
            assert_eq!(location, expected_location);
        }

        #[test]
        fn test_get_location_empty_traceback() {
            let traceback = "";
            let location = get_location(&Utf8PathBuf::new(), traceback);
            assert_eq!(location, None);
        }

        #[test]
        fn test_get_location_single_line() {
            let traceback = "Exception: Test error";
            let location = get_location(&Utf8PathBuf::new(), traceback);
            assert_eq!(location, None);
        }

        #[test]
        fn test_get_location_no_file_prefix() {
            let traceback = r"Traceback (most recent call last):
Some random line
Exception: Test error";
            let location = get_location(&Utf8PathBuf::new(), traceback);
            assert_eq!(location, None);
        }

        #[test]
        fn test_get_location_missing_line_number() {
            let traceback = r#"Traceback (most recent call last):
  File "test.py", in <module>
Exception: Test error"#;
            let location = get_location(&Utf8PathBuf::new(), traceback);
            assert_eq!(location, None);
        }

        #[test]
        fn test_get_location_malformed_quote() {
            let traceback = r#"Traceback (most recent call last):
  File "test.py, line 10, in <module>
Exception: Test error"#;
            let location = get_location(&Utf8PathBuf::new(), traceback);
            assert_eq!(location, None);
        }

        #[test]
        fn test_get_location_large_line_number() {
            let traceback = r#"Traceback (most recent call last):
  File "test.py", line 99999, in <module>
    code()
Exception: Test error"#;
            let location = get_location(&Utf8PathBuf::new(), traceback);
            let expected_location = Some(Location::new(
                "test.py".into(),
                OneIndexed::new(99999).unwrap(),
            ));
            assert_eq!(location, expected_location);
        }
    }
}
