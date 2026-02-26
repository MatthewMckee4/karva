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

    let mut result = String::new();
    let _ = writeln!(result, "success: {}", output.success);
    let _ = writeln!(result, "exit_code: {}", output.exit_code);
    result.push_str("----- stdout -----\n");
    result.push_str(&output.stdout);
    if !output.stdout.ends_with('\n') && !output.stdout.is_empty() {
        result.push('\n');
    }
    result.push_str("----- stderr -----\n");
    result.push_str(&output.stderr);
    if !output.stderr.is_empty() && !output.stderr.ends_with('\n') {
        result.push('\n');
    }
    result
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
}
