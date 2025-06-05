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
            writeln!(f, "{line_num:line_num_width$} | {line_content}",)?;
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
        writeln!(f, "{:width$} |", "", width = line_num_width)?;

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

    #[test]
    fn test_calculate_assertion_length() {
        assert_eq!(calculate_assertion_length("    assert True"), 4);
        assert_eq!(calculate_assertion_length("    assert x == y"), 6);
        assert_eq!(
            calculate_assertion_length("    assert False, \"message\""),
            16
        );
        assert_eq!(
            calculate_assertion_length("    assert len(items) > 5  # comment"),
            14
        );
    }
}
