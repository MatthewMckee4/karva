use std::fs;

use pyo3::{exceptions::PyAssertionError, prelude::*};
use ruff_python_ast::StmtFunctionDef;

use crate::diagnostic::render::DisplayDiagnostic;

pub mod render;
pub mod reporter;

// Public exports

#[derive(Clone)]
pub struct Diagnostic {
    diagnostic_type: DiagnosticType,
    info: String,
}

impl Diagnostic {
    pub fn from_py_err(error: &PyErr) -> Self {
        Self {
            diagnostic_type: DiagnosticType::Error,
            info: error.to_string(),
        }
    }

    pub fn from_fail(py: Python, _function_definition: StmtFunctionDef, error: &PyErr) -> Self {
        let err_value = error.value(py);
        if err_value.is_instance_of::<PyAssertionError>() {
            // Try to create a formatted assertion diagnostic
            if let Some(assertion_diag) = AssertionDiagnostic::from_assertion_error(py, error) {
                return Self {
                    diagnostic_type: DiagnosticType::Fail,
                    info: assertion_diag.display_formatted(),
                };
            }

            // Fallback to original behavior
            let traceback = error
                .traceback(py)
                .map(|traceback| filter_traceback(&traceback.format().unwrap_or_default()));
            Self {
                diagnostic_type: DiagnosticType::Fail,
                info: traceback.unwrap_or_default(),
            }
        } else {
            let traceback = error
                .traceback(py)
                .map(|traceback| filter_traceback(&traceback.format().unwrap_or_default()))
                .unwrap_or_default();
            Self {
                diagnostic_type: DiagnosticType::Error,
                info: traceback,
            }
        }
    }

    #[must_use]
    pub const fn diagnostic_type(&self) -> &DiagnosticType {
        &self.diagnostic_type
    }

    #[must_use]
    pub const fn display(&self) -> DisplayDiagnostic {
        DisplayDiagnostic::new(self)
    }
}

pub struct AssertionDiagnostic {
    file_path: String,
    line_number: usize,
    column: usize,
    source_line: String,
}

impl AssertionDiagnostic {
    /// Creates a new `AssertionDiagnostic` from a Python `AssertionError`.
    /// Returns None if the error is not an `AssertionError` or if parsing fails.
    pub fn from_assertion_error(py: Python, error: &PyErr) -> Option<Self> {
        let err_value = error.value(py);
        if !err_value.is_instance_of::<PyAssertionError>() {
            return None;
        }

        let traceback = error.traceback(py)?;
        let traceback_str = traceback.format().ok()?;

        // Parse the traceback to extract file, line, and column info
        if let Some((file_path, line_number, source_line)) = Self::parse_traceback(&traceback_str) {
            // Find the column position after "assert " (including space)
            let column = source_line.find("assert ").map_or_else(
                || source_line.find("assert").unwrap_or(0),
                |assert_pos| assert_pos + "assert ".len(),
            );

            Some(Self {
                file_path,
                line_number,
                column,
                source_line,
            })
        } else {
            None
        }
    }

    /// Creates a new `AssertionDiagnostic` with explicit values.
    /// Useful for testing or when you have the information already parsed.
    #[must_use]
    pub const fn new(
        file_path: String,
        line_number: usize,
        column: usize,
        source_line: String,
    ) -> Self {
        Self {
            file_path,
            line_number,
            column,
            source_line,
        }
    }

    fn parse_traceback(traceback: &str) -> Option<(String, usize, String)> {
        let lines: Vec<&str> = traceback.lines().collect();

        // Find the file line (starts with "  File ")
        for (i, line) in lines.iter().enumerate() {
            if line.trim().starts_with("File \"") && line.contains(", line ") {
                // Extract file path and line number
                let parts: Vec<&str> = line.split(", line ").collect();
                if parts.len() >= 2 {
                    let file_part = parts[0].trim();
                    let file_path = file_part
                        .strip_prefix("File \"")
                        .and_then(|s| s.strip_suffix("\""))
                        .unwrap_or("");

                    let line_num_part = parts[1].split(',').next().unwrap_or("0");
                    let line_number: usize = line_num_part.trim().parse().unwrap_or(0);

                    // The next line should contain the actual source code
                    // Keep original whitespace, don't trim
                    if i + 1 < lines.len() {
                        let source_line = lines[i + 1].to_string();
                        return Some((file_path.to_string(), line_number, source_line));
                    }
                }
            }
        }
        None
    }

    #[must_use]
    pub fn display_formatted(&self) -> String {
        let file_name = std::path::Path::new(&self.file_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&self.file_path);

        // Try to read the source file to get context lines
        let context_lines = self
            .get_context_lines()
            .unwrap_or_else(|| vec![self.source_line.clone()]);

        let mut output = String::new();
        output.push_str("error[assertion-failed]: Assertion failed\n");
        output.push_str(&format!(
            " --> {}:{}:{}\n",
            file_name,
            self.line_number,
            self.column + 1
        ));
        output.push_str("  |\n");

        // Display context lines
        let start_line = if self.line_number > 1 {
            self.line_number - 1
        } else {
            1
        };
        for (i, line) in context_lines.iter().enumerate() {
            let current_line_num = start_line + i;
            output.push_str(&format!("{current_line_num} | {line}\n"));
            if current_line_num == self.line_number {
                let line_num_width = current_line_num.to_string().len();
                let prefix_spaces = " ".repeat(line_num_width + 1);
                let caret_spaces = " ".repeat(self.column);

                // Calculate the length of the assertion statement after "assert"
                let assertion_length = Self::calculate_assertion_length(line);
                let carets = "^".repeat(assertion_length);

                output.push_str(&format!(
                    "{prefix_spaces}| {caret_spaces}{carets} assertion failed\n"
                ));
            }
        }
        output.push_str("  |\n");

        output
    }

    /// Calculate the length of the assertion statement after "assert"
    fn calculate_assertion_length(source_line: &str) -> usize {
        // Find the start of the assertion statement (after "assert ")
        let assert_start = if let Some(pos) = source_line.find("assert ") {
            pos + "assert ".len()
        } else if let Some(pos) = source_line.find("assert") {
            pos + "assert".len()
        } else {
            return 1; // Fallback to single caret
        };

        // Find the end of the assertion statement
        // This could be end of line, or before a comment
        let remaining = &source_line[assert_start..];
        let end_pos = remaining
            .find('#') // Stop at comment
            .unwrap_or(remaining.len()); // Or end of line

        // Trim whitespace from the end to get actual assertion length
        remaining[..end_pos].trim_end().len().max(1) // At least 1 caret
    }

    fn get_context_lines(&self) -> Option<Vec<String>> {
        let content = fs::read_to_string(&self.file_path).ok()?;
        let lines: Vec<&str> = content.lines().collect();

        if self.line_number == 0 || self.line_number > lines.len() {
            return None;
        }

        let start = if self.line_number > 1 {
            self.line_number - 2
        } else {
            0
        };
        let end = std::cmp::min(self.line_number, lines.len());

        Some(
            lines[start..end]
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
        )
    }
}

#[derive(Clone)]
pub enum DiagnosticType {
    Fail,
    Error,
}

fn filter_traceback(traceback: &str) -> String {
    let lines: Vec<&str> = traceback.lines().collect();
    let mut filtered = String::new();

    for (i, line) in lines.iter().enumerate() {
        if i == 0 && line.contains("Traceback (most recent call last):") {
            continue;
        }
        if line.starts_with("  ") {
            if let Some(stripped) = line.strip_prefix("  ") {
                filtered.push_str(stripped);
            }
        } else {
            filtered.push_str(line);
        }
        filtered.push('\n');
    }

    filtered.trim_end().to_string()
}
