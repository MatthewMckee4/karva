use std::borrow::Cow;

/// Output captured from running a command.
pub struct CommandOutput {
    pub success: bool,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Format command output in the insta-cmd style.
pub fn format_cmd_output(output: &CommandOutput) -> String {
    use std::fmt::Write;

    let stdout = normalize_line_endings(&output.stdout);
    let stderr = normalize_line_endings(&output.stderr);
    let mut result = String::new();
    let _ = writeln!(result, "success: {}", output.success);
    let _ = writeln!(result, "exit_code: {}", output.exit_code);
    result.push_str("----- stdout -----\n");
    result.push_str(&stdout);
    if !stdout.ends_with('\n') && !stdout.is_empty() {
        result.push('\n');
    }
    result.push_str("----- stderr -----\n");
    result.push_str(&stderr);
    if !stderr.is_empty() && !stderr.ends_with('\n') {
        result.push('\n');
    }
    result
}

fn normalize_line_endings(value: &str) -> Cow<'_, str> {
    if value.contains("\r\n") {
        Cow::Owned(value.replace("\r\n", "\n"))
    } else {
        Cow::Borrowed(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_with_stdout() {
        let output = CommandOutput {
            success: true,
            exit_code: 0,
            stdout: "hello\n".to_string(),
            stderr: String::new(),
        };
        insta::assert_snapshot!(format_cmd_output(&output), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        hello
        ----- stderr -----
        ");
    }

    #[test]
    fn failure_with_stderr() {
        let output = CommandOutput {
            success: false,
            exit_code: 1,
            stdout: String::new(),
            stderr: "error occurred\n".to_string(),
        };
        insta::assert_snapshot!(format_cmd_output(&output), @r"
        success: false
        exit_code: 1
        ----- stdout -----
        ----- stderr -----
        error occurred
        ");
    }

    #[test]
    fn both_stdout_and_stderr() {
        let output = CommandOutput {
            success: true,
            exit_code: 0,
            stdout: "output\n".to_string(),
            stderr: "warnings\n".to_string(),
        };
        insta::assert_snapshot!(format_cmd_output(&output), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        output
        ----- stderr -----
        warnings
        ");
    }

    #[test]
    fn empty_output() {
        let output = CommandOutput {
            success: true,
            exit_code: 0,
            stdout: String::new(),
            stderr: String::new(),
        };
        insta::assert_snapshot!(format_cmd_output(&output), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        ----- stderr -----
        ");
    }

    #[test]
    fn stdout_no_trailing_newline() {
        let output = CommandOutput {
            success: true,
            exit_code: 0,
            stdout: "no newline".to_string(),
            stderr: String::new(),
        };
        insta::assert_snapshot!(format_cmd_output(&output), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        no newline
        ----- stderr -----
        ");
    }

    #[test]
    fn nonzero_exit_code() {
        let output = CommandOutput {
            success: false,
            exit_code: 42,
            stdout: String::new(),
            stderr: "exit 42\n".to_string(),
        };
        insta::assert_snapshot!(format_cmd_output(&output), @r"
        success: false
        exit_code: 42
        ----- stdout -----
        ----- stderr -----
        exit 42
        ");
    }

    #[test]
    fn stderr_no_trailing_newline() {
        let output = CommandOutput {
            success: false,
            exit_code: 1,
            stdout: String::new(),
            stderr: "error without newline".to_string(),
        };
        insta::assert_snapshot!(format_cmd_output(&output), @r"
        success: false
        exit_code: 1
        ----- stdout -----
        ----- stderr -----
        error without newline
        ");
    }

    #[test]
    fn both_no_trailing_newline() {
        let output = CommandOutput {
            success: true,
            exit_code: 0,
            stdout: "out".to_string(),
            stderr: "err".to_string(),
        };
        insta::assert_snapshot!(format_cmd_output(&output), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        out
        ----- stderr -----
        err
        ");
    }

    #[test]
    fn normalizes_crlf_line_endings() {
        let output = CommandOutput {
            success: true,
            exit_code: 0,
            stdout: "first\r\nsecond\r\n".to_string(),
            stderr: "warning\r\n".to_string(),
        };
        insta::assert_snapshot!(format_cmd_output(&output), @r"
        success: true
        exit_code: 0
        ----- stdout -----
        first
        second
        ----- stderr -----
        warning
        ");
    }
}
