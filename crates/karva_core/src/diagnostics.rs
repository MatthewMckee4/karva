use colored::Colorize;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use crate::runner::RunnerResult;

pub trait DiagnosticWriter: Send + Sync {
    /// Called when a test starts running
    fn test_started(&self, test_name: &str, file_path: &str);

    /// Called when a test completes
    fn test_completed(&self, test_name: &str, file_path: &str, passed: bool);

    /// Called when a test fails with an error message
    fn test_error(&self, test_name: &str, file_path: &str, error: &str);

    /// Called when test discovery starts
    fn discovery_started(&self);

    /// Called when test discovery completes
    fn discovery_completed(&self, count: usize);

    /// Flush all output to stdout
    fn finish(&self, runner_result: &RunnerResult);
}

pub struct StdoutDiagnosticWriter {
    stdout: Arc<Mutex<Box<dyn Write + Send>>>,
}

impl Default for StdoutDiagnosticWriter {
    fn default() -> Self {
        Self::new(io::stdout())
    }
}

impl StdoutDiagnosticWriter {
    pub fn new(out: impl Write + Send + 'static) -> Self {
        Self {
            stdout: Arc::new(Mutex::new(Box::new(out))),
        }
    }

    fn acquire_stdout(&self) -> std::sync::MutexGuard<'_, Box<dyn Write + Send>> {
        self.stdout.lock().unwrap()
    }

    fn flush_stdout(&self, stdout: &mut std::sync::MutexGuard<'_, Box<dyn Write + Send>>) {
        let _ = stdout.flush();
    }
}

impl DiagnosticWriter for StdoutDiagnosticWriter {
    fn test_started(&self, test_name: &str, file_path: &str) {
        tracing::debug!("{} {} in {}", "Running".blue(), test_name, file_path);
    }

    fn test_completed(&self, test_name: &str, file_path: &str, passed: bool) {
        let mut stdout = self.acquire_stdout();
        if passed {
            tracing::debug!("{} {} in {}", "Passed".green(), test_name, file_path);
            let _ = write!(stdout, "{}", ".".green());
        } else {
            tracing::debug!("{} {} in {}", "Failed".red(), test_name, file_path);
            let _ = write!(stdout, "{}", ".".red());
        }
        self.flush_stdout(&mut stdout);
    }

    fn test_error(&self, test_name: &str, file_path: &str, error: &str) {
        let mut stdout = self.acquire_stdout();
        let _ = writeln!(
            stdout,
            "{} {} in {}: {}",
            "Error".red().bold(),
            test_name,
            file_path,
            error
        );
        self.flush_stdout(&mut stdout);
    }

    fn discovery_started(&self) {
        tracing::debug!("{}", "Discovering tests...".blue());
    }

    fn discovery_completed(&self, count: usize) {
        let mut stdout = self.acquire_stdout();
        let _ = writeln!(
            stdout,
            "{} {} {}",
            "Discovered".blue(),
            count,
            "tests".blue()
        );
        self.flush_stdout(&mut stdout);
    }

    fn finish(&self, runner_result: &RunnerResult) {
        let mut stdout = self.acquire_stdout();
        let stats = runner_result.stats();
        let _ = writeln!(stdout);
        let _ = writeln!(
            stdout,
            "{} {}",
            "Passed tests:".green(),
            stats.passed_tests()
        );
        let _ = writeln!(stdout, "{} {}", "Failed tests:".red(), stats.failed_tests());
        let _ = writeln!(
            stdout,
            "{} {}",
            "Error tests:".yellow(),
            stats.error_tests()
        );
        self.flush_stdout(&mut stdout);
    }
}
