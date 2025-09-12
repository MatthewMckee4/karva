use pyo3::prelude::*;

pub(crate) fn get_traceback(py: Python<'_>, error: &PyErr) -> String {
    if let Some(traceback) = error.traceback(py) {
        let traceback_str = traceback.format().unwrap_or_default();
        if traceback_str.is_empty() {
            return error.to_string();
        }
        filter_traceback(&traceback_str)
            .lines()
            .map(|line| format!(" | {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        error.to_string()
    }
}

pub(crate) fn to_kebab_case(input: &str) -> String {
    input
        .chars()
        .enumerate()
        .fold(String::new(), |mut acc, (i, c)| {
            if i > 0 && c.is_uppercase() {
                acc.push('-');
            }
            acc.push(c.to_ascii_lowercase());
            acc
        })
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
    mod to_kebab_case_tests {
        use super::*;

        #[test]
        fn test_kebab_case_conversion() {
            assert_eq!(to_kebab_case("SpecialError"), "special-error");
            assert_eq!(to_kebab_case("ValueError"), "value-error");
            assert_eq!(to_kebab_case("DeprecationWarning"), "deprecation-warning");
            assert_eq!(to_kebab_case("UnicodeWarning"), "unicode-warning");
            assert_eq!(to_kebab_case("TestWarning"), "test-warning");
            assert_eq!(to_kebab_case("SomeComplexWarning"), "some-complex-warning");
        }
    }

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
