use pyo3::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Traceback {
    pub(crate) lines: Vec<String>,

    pub(crate) location: Option<String>,
}

impl Traceback {
    pub(crate) fn new(py: Python<'_>, error: &PyErr) -> Self {
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
                location: get_location(&traceback_str),
            }
        } else {
            Self {
                lines: vec![],
                location: None,
            }
        }
    }
}

fn get_location(traceback: &str) -> Option<String> {
    let lines: Vec<&str> = traceback.lines().collect();
    let second_last_line = lines.get(lines.len() - 2).unwrap_or(&"");
    if let Some(after_file) = second_last_line.strip_prefix("File \"") {
        if let Some(quote_end) = after_file.find('"') {
            let filename = &after_file[..quote_end];
            let rest = &after_file[quote_end + 1..];
            if let Some(line_start) = rest.find("line ") {
                let line_part = &rest[line_start + 5..];
                if let Some(comma_pos) = line_part.find(',') {
                    let line_number = &line_part[..comma_pos];
                    return Some(format!("{filename}:{line_number}"));
                }
            }
        }
    }

    None
}

// Simplified traceback filtering that removes unnecessary traceback headers
pub(crate) fn filter_traceback(traceback: &str) -> String {
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
}
