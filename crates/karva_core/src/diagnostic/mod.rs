use pyo3::{exceptions::PyAssertionError, prelude::*};
use ruff_python_ast::StmtFunctionDef;

use crate::{
    diagnostic::render::{DisplayAssertionDiagnostic, DisplayDiagnostic},
    discovery::module::Module,
};

pub mod render;
pub mod reporter;

// Type aliases for cleaner function signatures
type AssertionLineInfo = (usize, usize, String); // (line_number, column, source_line)
type ContextLines = Vec<(usize, String)>; // Vec<(line_number, line_content)>

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

    pub fn from_fail(
        py: Python,
        module: &Module,
        function_definition: &StmtFunctionDef,
        error: &PyErr,
    ) -> Self {
        let err_value = error.value(py);
        if err_value.is_instance_of::<PyAssertionError>() {
            if let Some(assertion_diagnostic) =
                AssertionDiagnostic::from_assertion_error_with_module(
                    py,
                    error,
                    module,
                    function_definition,
                )
            {
                return Self {
                    diagnostic_type: DiagnosticType::Fail,
                    info: assertion_diagnostic.display().to_string(),
                };
            }

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
    context_lines: Vec<(usize, String)>, // (line_number, line_content) pairs for context
    function_name: String,
}

impl AssertionDiagnostic {
    // Creates a new AssertionDiagnostic using Module's position tracking capabilities
    // This leverages the Module's to_column_row method for accurate position information
    pub fn from_assertion_error_with_module(
        py: Python,
        error: &PyErr,
        module: &Module,
        function_def: &StmtFunctionDef,
    ) -> Option<Self> {
        let err_value = error.value(py);
        if !err_value.is_instance_of::<PyAssertionError>() {
            return None;
        }

        // Get the traceback to find the exact file and line where the assertion failed
        let traceback = error.traceback(py)?;
        let traceback_str = traceback.format().ok()?;
        let (failed_file_path, failed_line_number) =
            Self::extract_file_and_line_from_traceback(&traceback_str)?;

        // Determine if the assertion failed in the same file as the test function
        let module_file_path = module.file().as_std_path().to_string_lossy().to_string();
        let module_file_name = std::path::Path::new(&module_file_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&module_file_path);
        let failed_file_name = std::path::Path::new(&failed_file_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&failed_file_path);

        let (source_content, display_file_path) = if module_file_name == failed_file_name {
            // Assertion failed in the same file as the test function
            (module.source_text(), module_file_path)
        } else {
            // Assertion failed in a different file - read that file
            std::fs::read_to_string(&failed_file_path).map_or_else(
                |_| (module.source_text(), module_file_path),
                |content| (content, failed_file_path),
            )
        };

        let (assertion_line_info, context_lines) =
            Self::get_assertion_line_info_with_context(&source_content, failed_line_number)?;

        Some(Self {
            file_path: display_file_path,
            line_number: assertion_line_info.0,
            column: assertion_line_info.1,
            source_line: assertion_line_info.2,
            context_lines,
            function_name: function_def.name.to_string(),
        })
    }

    // Extract both the file path and line number from Python traceback
    // Returns the deepest call stack entry where the assertion actually failed
    fn extract_file_and_line_from_traceback(traceback: &str) -> Option<(String, usize)> {
        let mut last_file_info = None;

        for line in traceback.lines() {
            if line.trim().starts_with("File \"") && line.contains(", line ") {
                let parts: Vec<&str> = line.split(", line ").collect();
                if parts.len() >= 2 {
                    let file_part = parts[0].trim();
                    let file_path = file_part
                        .strip_prefix("File \"")
                        .and_then(|s| s.strip_suffix("\""))
                        .unwrap_or("");

                    let line_num_part = parts[1].split(',').next().unwrap_or("0");
                    if let Ok(line_number) = line_num_part.trim().parse::<usize>() {
                        last_file_info = Some((file_path.to_string(), line_number));
                    }
                }
            }
        }

        last_file_info
    }

    // Get the assertion line information with up to 5 lines of context above it
    // Only includes lines that are within the function body
    fn get_assertion_line_info_with_context(
        source_content: &str,
        line_number: usize,
    ) -> Option<(AssertionLineInfo, ContextLines)> {
        let lines: Vec<&str> = source_content.lines().collect();
        let assertion_line_idx = line_number.saturating_sub(1); // Convert to 0-indexed

        if assertion_line_idx >= lines.len() {
            return None;
        }

        let assertion_line = lines[assertion_line_idx];

        // Find the column position of the assertion
        let column = assertion_line.find("assert ").map_or_else(
            || assertion_line.find("assert").unwrap_or(0),
            |assert_pos| assert_pos + "assert ".len(),
        );

        // Find context lines - up to 5 lines above, but start from function definition
        let mut context_lines = Vec::new();
        let start_idx = if assertion_line_idx >= 5 {
            assertion_line_idx - 5
        } else {
            0
        };

        for (i, line) in lines
            .iter()
            .enumerate()
            .take(assertion_line_idx)
            .skip(start_idx)
        {
            let line_num = i + 1;

            // If we hit a function definition, clear previous context and start fresh
            if line.trim_start().starts_with("def ") {
                context_lines.clear(); // Clear previous context
            }

            // Include all lines after we start collecting (including the function def)
            context_lines.push((line_num, (*line).to_string()));
        }

        // Limit to last 5 context lines to keep output manageable
        if context_lines.len() > 5 {
            let start_idx = context_lines.len() - 5;
            context_lines.drain(0..start_idx);
        }

        Some((
            (line_number, column, assertion_line.to_string()),
            context_lines,
        ))
    }

    // Legacy method for creating diagnostics (for tests)
    #[must_use]
    pub const fn new(
        file_path: String,
        line_number: usize,
        column: usize,
        source_line: String,
        function_name: String,
    ) -> Self {
        Self {
            file_path,
            line_number,
            column,
            source_line,
            context_lines: Vec::new(), // No context for manually created diagnostics
            function_name,
        }
    }

    #[must_use]
    pub const fn display(&self) -> DisplayAssertionDiagnostic {
        DisplayAssertionDiagnostic::new(self)
    }
}

#[derive(Clone)]
pub enum DiagnosticType {
    Fail,
    Error,
}

// Simplified traceback filtering that removes unnecessary traceback headers
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_formatted() {
        let diagnostic = AssertionDiagnostic::new(
            "/path/to/test.py".to_string(),
            42,
            11,
            "    assert x == y".to_string(),
            "test_equality".to_string(),
        );

        let output = diagnostic.display().to_string();
        assert!(output.contains("error[assertion-failed]: Assertion failed"));
        assert!(output.contains("test.py:42:12 in function 'test_equality'"));
        assert!(output.contains("42 |     assert x == y"));
        assert!(output.contains("   |            ^^^^^^ assertion failed"));
    }

    #[test]
    fn test_filter_traceback() {
        let traceback = r#"Traceback (most recent call last):
  File "test.py", line 3, in test_func
    assert False
AssertionError"#;

        let filtered = filter_traceback(traceback);
        let expected = r#"File "test.py", line 3, in test_func
  assert False
AssertionError"#;
        assert_eq!(filtered, expected);
    }

    #[test]
    fn test_extract_file_and_line_from_traceback() {
        let traceback = r#"Traceback (most recent call last):
  File "/path/to/test.py", line 25, in test_function
    helper_function()
  File "/path/to/helper.py", line 10, in helper_function
    assert x == y
AssertionError"#;

        let result = AssertionDiagnostic::extract_file_and_line_from_traceback(traceback);
        assert_eq!(result, Some(("/path/to/helper.py".to_string(), 10)));
    }

    #[test]
    fn test_extract_file_and_line_single_file() {
        let traceback = r#"Traceback (most recent call last):
  File "/path/to/test.py", line 42, in test_function
    assert False
AssertionError"#;

        let result = AssertionDiagnostic::extract_file_and_line_from_traceback(traceback);
        assert_eq!(result, Some(("/path/to/test.py".to_string(), 42)));
    }
}
