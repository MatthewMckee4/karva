use std::{
    path::{Path, PathBuf},
    process::Command,
};

use crate::TestContext;

pub struct IntegrationTestContext {
    test_env: TestContext,
}

impl Default for IntegrationTestContext {
    fn default() -> Self {
        Self::new()
    }
}

impl IntegrationTestContext {
    #[must_use]
    pub fn new() -> Self {
        let test_env = TestContext::new();

        Self { test_env }
    }

    #[must_use]
    pub fn karva_bin(&self) -> PathBuf {
        let venv_bin =
            self.test_env
                .cwd()
                .join(".venv")
                .join(if cfg!(windows) { "Scripts" } else { "bin" });
        venv_bin.join(if cfg!(windows) { "karva.exe" } else { "karva" })
    }

    pub fn with_files<'a>(
        files: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> anyhow::Result<Self> {
        let mut case = Self::new();
        case.write_files(files)?;
        Ok(case)
    }

    pub fn with_file(path: impl AsRef<Path>, content: &str) -> anyhow::Result<Self> {
        let mut case = Self::new();
        case.write_file(path, content)?;
        Ok(case)
    }

    pub fn write_files<'a>(
        &mut self,
        files: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> anyhow::Result<()> {
        for (path, content) in files {
            self.write_file(path, content)?;
        }

        Ok(())
    }

    pub fn write_file(&mut self, path: impl AsRef<Path>, content: &str) -> anyhow::Result<()> {
        self.test_env.write_file(path, content)
    }

    #[must_use]
    pub fn command(&self) -> Command {
        let mut command = Command::new(self.karva_bin());
        command.current_dir(self.test_env.cwd()).arg("test");
        command
    }

    #[must_use]
    pub fn command_with_args(&self, args: &[&str]) -> Command {
        let mut command = self.command();
        command.args(args);
        command
    }

    /// Execute command and return normalized output string for snapshot testing.
    /// This normalizes test output ordering to handle non-deterministic test execution.
    #[must_use]
    pub fn command_snapshot(&self) -> String {
        let output = self.command().output().expect("Failed to execute command");
        format_output(&output)
    }

    /// Execute command with args and return normalized output string for snapshot testing.
    /// This normalizes test output ordering to handle non-deterministic test execution.
    #[must_use]
    pub fn command_with_args_snapshot(&self, args: &[&str]) -> String {
        let output = self
            .command_with_args(args)
            .output()
            .expect("Failed to execute command");
        format_output(&output)
    }
}

fn format_output(output: &std::process::Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let normalized_stdout = normalize_test_output(&stdout);

    format!(
        "success: {}\nexit_code: {}\n----- stdout -----\n{}\n----- stderr -----\n{}",
        output.status.success(),
        output.status.code().unwrap_or(-1),
        normalized_stdout,
        stderr
    )
}

/// Sorts test result lines in snapshot output to handle non-deterministic test execution order.
/// This ensures snapshots remain stable even when tests run in different orders.
#[must_use]
pub fn normalize_test_output(output: &str) -> String {
    let mut lines: Vec<&str> = output.lines().collect();

    // Find where test results start
    let test_start = lines.iter().position(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("test ")
            && (trimmed.contains("... ok")
                || trimmed.contains("... FAILED")
                || trimmed.contains("... skipped"))
    });

    if let Some(start) = test_start {
        // Find where test results end - look for first non-empty, non-test line
        let test_end = lines[start..]
            .iter()
            .position(|line| {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    return false; // Skip empty lines, they're part of the test output
                }
                // Stop when we hit something that's not a test result line
                !(trimmed.starts_with("test ")
                    && (trimmed.contains("... ok")
                        || trimmed.contains("... FAILED")
                        || trimmed.contains("... skipped")))
            })
            .map_or(lines.len(), |idx| start + idx);

        // Sort only the test result lines
        lines[start..(test_end - 1)].sort_unstable();
    }

    lines.join("\n")
}
