use crate::diagnostic::{AssertionDiagnostic, Diagnostic};

pub struct DisplayDiagnostic<'a> {
    diagnostic: &'a Diagnostic,
}

impl<'a> DisplayDiagnostic<'a> {
    #[must_use]
    pub const fn new(diagnostic: &'a Diagnostic) -> Self {
        Self { diagnostic }
    }
}

impl std::fmt::Display for DisplayDiagnostic<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.diagnostic.info)
    }
}

pub struct DisplayAssertionDiagnostic<'a> {
    diagnostic: &'a AssertionDiagnostic,
}

impl<'a> DisplayAssertionDiagnostic<'a> {
    #[must_use]
    pub const fn new(diagnostic: &'a AssertionDiagnostic) -> Self {
        Self { diagnostic }
    }
}

impl std::fmt::Display for DisplayAssertionDiagnostic<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let file_name = std::path::Path::new(&self.diagnostic.file_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&self.diagnostic.file_path);

        writeln!(f, "error[assertion-failed]: Assertion failed")?;
        writeln!(
            f,
            " --> {}:{}:{} in function '{}'",
            file_name,
            self.diagnostic.line_number,
            self.diagnostic.column + 1,
            self.diagnostic.function_name
        )?;

        // Calculate width needed for line number alignment (consider all lines)
        let max_line_num = if self.diagnostic.context_lines.is_empty() {
            self.diagnostic.line_number
        } else {
            self.diagnostic
                .context_lines
                .iter()
                .map(|(num, _)| *num)
                .chain(std::iter::once(self.diagnostic.line_number))
                .max()
                .unwrap_or(self.diagnostic.line_number)
        };
        let line_num_width = max_line_num.to_string().len();

        // Top border: spaces equal to line number width, then " |"
        writeln!(f, "{:width$} |", "", width = line_num_width)?;

        // Show context lines first
        for (line_num, line_content) in &self.diagnostic.context_lines {
            if line_content.is_empty() {
                writeln!(f, "{line_num:line_num_width$} |")?;
            } else {
                writeln!(f, "{line_num:line_num_width$} | {line_content}")?;
            }
        }

        // Show the assertion line
        writeln!(
            f,
            "{:width$} | {}",
            self.diagnostic.line_number,
            self.diagnostic.source_line,
            width = line_num_width
        )?;

        // Error indicator line: spaces equal to line number width, then " | ", then column spaces, then carets
        write!(f, "{:width$} | ", "", width = line_num_width)?;
        write!(f, "{:width$}", "", width = self.diagnostic.column)?;

        let assertion_length = calculate_assertion_length(&self.diagnostic.source_line);
        write!(f, "{:^<width$}", "", width = assertion_length)?;
        writeln!(f, " assertion failed")?;

        // Bottom border: spaces equal to line number width, then " |"
        writeln!(f, "{:line_num_width$} |", "")?;

        Ok(())
    }
}

// Calculate the length of the assertion statement after "assert"
// Used for drawing the correct number of carets under the assertion
fn calculate_assertion_length(source_line: &str) -> usize {
    let assert_start = if let Some(pos) = source_line.find("assert ") {
        pos + "assert ".len()
    } else if let Some(pos) = source_line.find("assert") {
        pos + "assert".len()
    } else {
        return 1;
    };

    let remaining = &source_line[assert_start..];
    let end_pos = remaining.find('#').unwrap_or(remaining.len());

    remaining[..end_pos].trim_end().len().max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostic::{AssertionDiagnostic, Diagnostic, DiagnosticType};

    fn create_diagnostic(diagnostic_type: DiagnosticType, info: &str) -> Diagnostic {
        Diagnostic {
            diagnostic_type,
            info: info.to_string(),
        }
    }

    fn create_assertion_diagnostic(
        file_path: &str,
        line_number: usize,
        column: usize,
        source_line: &str,
        function_name: &str,
        context_lines: Vec<(usize, String)>,
    ) -> AssertionDiagnostic {
        AssertionDiagnostic {
            file_path: file_path.to_string(),
            line_number,
            column,
            source_line: source_line.to_string(),
            context_lines,
            function_name: function_name.to_string(),
        }
    }

    #[test]
    fn test_display_diagnostic_error_type() {
        let diagnostic = create_diagnostic(DiagnosticType::Error, "Test error message");
        let display = diagnostic.display();
        assert_eq!(display.to_string(), "Test error message");
    }

    #[test]
    fn test_display_diagnostic_fail_type() {
        let diagnostic = create_diagnostic(DiagnosticType::Fail, "Test failure message");
        let display = diagnostic.display();
        assert_eq!(display.to_string(), "Test failure message");
    }

    #[test]
    fn test_display_diagnostic_empty_info() {
        let diagnostic = create_diagnostic(DiagnosticType::Error, "");
        let display = diagnostic.display();
        assert_eq!(display.to_string(), "");
    }

    #[test]
    fn test_display_diagnostic_multiline_info() {
        let diagnostic = create_diagnostic(DiagnosticType::Fail, "Line 1\nLine 2\nLine 3");
        let display = diagnostic.display();
        assert_eq!(display.to_string(), "Line 1\nLine 2\nLine 3");
    }

    #[test]
    fn test_display_diagnostic_long_info() {
        let long_info = "A".repeat(1000);
        let diagnostic = create_diagnostic(DiagnosticType::Error, &long_info);
        let display = diagnostic.display();
        assert_eq!(display.to_string(), long_info);
    }

    #[test]
    fn test_display_assertion_diagnostic_basic() {
        let diagnostic = create_assertion_diagnostic(
            "/path/to/test.py",
            42,
            11,
            "    assert x == y",
            "test_equality",
            vec![],
        );

        let output = diagnostic.display().to_string();
        assert!(output.contains("error[assertion-failed]: Assertion failed"));
        assert!(output.contains("test.py:42:12 in function 'test_equality'"));
        assert!(output.contains("42 |     assert x == y"));
        assert!(output.contains("   |            ^^^^^^ assertion failed"));
    }

    #[test]
    fn test_display_assertion_diagnostic_with_context() {
        let context_lines = vec![
            (1, "def test_function():".to_string()),
            (2, "    x = 5".to_string()),
            (3, "    y = 10".to_string()),
        ];

        let diagnostic = create_assertion_diagnostic(
            "/path/to/test.py",
            4,
            11,
            "    assert x == y",
            "test_function",
            context_lines,
        );

        let output = diagnostic.display().to_string();
        assert!(output.contains("1 | def test_function():"));
        assert!(output.contains("2 |     x = 5"));
        assert!(output.contains("3 |     y = 10"));
        assert!(output.contains("4 |     assert x == y"));
    }

    #[test]
    fn test_display_assertion_diagnostic_with_empty_context_line() {
        let context_lines = vec![
            (1, "def test_function():".to_string()),
            (2, "    x = 5".to_string()),
            (3, String::new()),
            (4, "    y = 10".to_string()),
        ];

        let diagnostic = create_assertion_diagnostic(
            "/path/to/test.py",
            5,
            11,
            "    assert x == y",
            "test_function",
            context_lines,
        );

        let output = diagnostic.display().to_string();
        assert!(output.contains("1 | def test_function():"));
        assert!(output.contains("2 |     x = 5"));
        assert!(output.contains("3 |"));
        assert!(!output.contains("3 | "));
        assert!(output.contains("4 |     y = 10"));
        assert!(output.contains("5 |     assert x == y"));
    }

    #[test]
    fn test_display_assertion_diagnostic_line_number_alignment() {
        let context_lines = vec![
            (8, "def test_function():".to_string()),
            (9, "    x = 5".to_string()),
            (10, "    y = 10".to_string()),
        ];

        let diagnostic = create_assertion_diagnostic(
            "/path/to/test.py",
            100,
            11,
            "    assert x == y",
            "test_function",
            context_lines,
        );

        let output = diagnostic.display().to_string();
        assert!(output.contains("  8 | def test_function():"));
        assert!(output.contains("  9 |     x = 5"));
        assert!(output.contains(" 10 |     y = 10"));
        assert!(output.contains("100 |     assert x == y"));
    }

    #[test]
    fn test_display_assertion_diagnostic_different_file_extensions() {
        let test_cases = vec![
            ("/path/to/test.py", "test.py"),
            ("/path/to/module.py", "module.py"),
            ("/path/to/script.pyi", "script.pyi"),
            ("test.py", "test.py"),
            ("./test.py", "test.py"),
            ("../test.py", "test.py"),
        ];

        for (file_path, expected_name) in test_cases {
            let diagnostic = create_assertion_diagnostic(
                file_path,
                42,
                11,
                "    assert True",
                "test_func",
                vec![],
            );

            let output = diagnostic.display().to_string();
            assert!(output.contains(&format!("{expected_name}:42:12")));
        }
    }

    #[test]
    fn test_display_assertion_diagnostic_different_function_names() {
        let test_cases = vec![
            "test_simple",
            "test_with_underscore",
            "TestClass.test_method",
            "very_long_function_name_that_might_cause_issues",
            "test123",
            "test_with_numbers_123",
        ];

        for function_name in test_cases {
            let diagnostic = create_assertion_diagnostic(
                "/path/to/test.py",
                42,
                11,
                "    assert True",
                function_name,
                vec![],
            );

            let output = diagnostic.display().to_string();
            assert!(output.contains(&format!("in function '{function_name}'")));
        }
    }

    #[test]
    fn test_display_assertion_diagnostic_different_columns() {
        let test_cases = vec![
            (8, "assert True", "       ^^^^ assertion failed"),
            (12, "    assert True", "           ^^^^ assertion failed"),
            (
                16,
                "        assert True",
                "               ^^^^ assertion failed",
            ),
            (
                20,
                "            assert x == y",
                "                   ^^^^^^ assertion failed",
            ),
        ];

        for (column, source_line, expected_caret_line) in test_cases {
            let diagnostic = create_assertion_diagnostic(
                "/path/to/test.py",
                42,
                column,
                source_line,
                "test_func",
                vec![],
            );

            let output = diagnostic.display().to_string();
            assert!(output.contains(expected_caret_line));
        }
    }

    #[test]
    fn test_display_assertion_diagnostic_large_line_numbers() {
        let diagnostic = create_assertion_diagnostic(
            "/path/to/test.py",
            9999,
            11,
            "    assert False",
            "test_func",
            vec![(9998, "    # Previous line".to_string())],
        );

        let output = diagnostic.display().to_string();
        assert!(output.contains("9998 |     # Previous line"));
        assert!(output.contains("9999 |     assert False"));
        assert!(output.contains("test.py:9999:12"));
    }

    #[test]
    fn test_display_assertion_diagnostic_long_source_line() {
        let long_assertion = format!("    assert {}", "x == y && ".repeat(20));
        let diagnostic = create_assertion_diagnostic(
            "/path/to/test.py",
            42,
            11,
            &long_assertion,
            "test_func",
            vec![],
        );

        let output = diagnostic.display().to_string();
        assert!(output.contains(&long_assertion));
        assert!(output.contains("assertion failed"));
    }

    #[test]
    fn test_display_assertion_diagnostic_no_context() {
        let diagnostic = create_assertion_diagnostic(
            "/path/to/test.py",
            1,
            0,
            "assert False",
            "test_func",
            vec![],
        );

        let output = diagnostic.display().to_string();
        assert!(output.contains("1 | assert False"));
        let line_count = output.lines().count();
        assert_eq!(line_count, 6);
    }

    #[test]
    fn test_calculate_assertion_length_basic() {
        assert_eq!(calculate_assertion_length("    assert True"), 4);
        assert_eq!(calculate_assertion_length("    assert False"), 5);
        assert_eq!(calculate_assertion_length("assert x"), 1);
    }

    #[test]
    fn test_calculate_assertion_length_with_spaces() {
        assert_eq!(calculate_assertion_length("    assert x == y"), 6);
        assert_eq!(calculate_assertion_length("assert     x == y"), 10);
        assert_eq!(calculate_assertion_length("    assert x == y   "), 6); // Trailing spaces trimmed
    }

    #[test]
    fn test_calculate_assertion_length_with_message() {
        assert_eq!(
            calculate_assertion_length("    assert False, \"message\""),
            16
        );
        assert_eq!(calculate_assertion_length("assert x == y, 'error'"), 15);
        assert_eq!(
            calculate_assertion_length("assert condition, f'Value: {value}'"),
            28
        );
    }

    #[test]
    fn test_calculate_assertion_length_with_comments() {
        assert_eq!(calculate_assertion_length("    assert True  # comment"), 4);
        assert_eq!(
            calculate_assertion_length("    assert x == y # this should pass"),
            6
        );
        assert_eq!(calculate_assertion_length("assert False# no space"), 5);
    }

    #[test]
    fn test_calculate_assertion_length_complex_expressions() {
        assert_eq!(calculate_assertion_length("    assert len(items) > 0"), 14);
        assert_eq!(calculate_assertion_length("assert callable(func)"), 14);
        assert_eq!(
            calculate_assertion_length("assert isinstance(obj, MyClass)"),
            24
        );
        assert_eq!(
            calculate_assertion_length("assert all(x > 0 for x in values)"),
            26
        );
    }

    #[test]
    fn test_calculate_assertion_length_no_space_after_assert() {
        assert_eq!(calculate_assertion_length("    assert(True)"), 6);
        assert_eq!(calculate_assertion_length("assert[condition]"), 11);
        assert_eq!(calculate_assertion_length("assertFalse"), 5);
    }

    #[test]
    fn test_calculate_assertion_length_no_assert() {
        assert_eq!(calculate_assertion_length("    something else"), 1);
        assert_eq!(calculate_assertion_length(""), 1);
        assert_eq!(calculate_assertion_length("    # just a comment"), 1);
    }

    #[test]
    fn test_calculate_assertion_length_multiline_string() {
        assert_eq!(
            calculate_assertion_length("    assert \"multi\\nline\""),
            13
        );
        assert_eq!(calculate_assertion_length("assert '''triple quotes'''"), 19);
    }

    #[test]
    fn test_calculate_assertion_length_edge_cases() {
        assert_eq!(calculate_assertion_length("assert"), 1);
        assert_eq!(calculate_assertion_length("assert "), 1);
        assert_eq!(calculate_assertion_length("    assert# comment"), 1);
    }

    #[test]
    fn test_display_assertion_diagnostic_full_scenario() {
        let context_lines = vec![
            (15, "def test_complex_scenario():".to_string()),
            (16, "    # Setup test data".to_string()),
            (17, "    data = [1, 2, 3, 4, 5]".to_string()),
            (18, "    expected = 6".to_string()),
            (19, String::new()),
            (20, "    # Perform calculation".to_string()),
        ];

        let diagnostic = create_assertion_diagnostic(
            "/home/user/projects/my_project/tests/test_calculations.py",
            21,
            11,
            "    assert sum(data) == expected, f'Expected {expected}, got {sum(data)}'",
            "test_complex_scenario",
            context_lines,
        );

        let output = diagnostic.display().to_string();

        assert!(output.contains("error[assertion-failed]: Assertion failed"));
        assert!(output.contains("test_calculations.py:21:12 in function 'test_complex_scenario'"));

        assert!(output.contains("15 | def test_complex_scenario():"));
        assert!(output.contains("16 |     # Setup test data"));
        assert!(output.contains("17 |     data = [1, 2, 3, 4, 5]"));
        assert!(output.contains("18 |     expected = 6"));
        assert!(output.contains("19 |"));
        assert!(output.contains("20 |     # Perform calculation"));

        assert!(output.contains(
            "21 |     assert sum(data) == expected, f'Expected {expected}, got {sum(data)}'"
        ));
        assert!(output.contains("   |            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ assertion failed"));

        assert!(!output.contains(" 15 |"));
        assert!(output.contains("15 |"));
    }

    #[test]
    fn test_display_assertion_diagnostic_minimal_scenario() {
        let diagnostic = create_assertion_diagnostic("test.py", 1, 0, "assert 0", "test", vec![]);

        let output = diagnostic.display().to_string();

        assert!(output.contains("error[assertion-failed]: Assertion failed"));
        assert!(output.contains("test.py:1:1 in function 'test'"));
        assert!(output.contains("1 | assert 0"));
        assert!(output.contains("  | ^ assertion failed"));
    }
}
