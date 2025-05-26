use colored::Colorize;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::runner::RunnerResult;

pub trait DiagnosticWriter: Send + Sync {
    /// Called when a test starts running
    fn test_started(&self, test_name: &str, file_path: &str);

    /// Called when a test completes
    fn test_completed(
        &self,
        test_name: &str,
        file_path: &str,
        passed: bool,
        duration: std::time::Duration,
    );

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
    start_time: Instant,
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
            start_time: Instant::now(),
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
        tracing::debug!(
            "{} {} in {} at {}ms",
            "Running".blue(),
            test_name,
            file_path,
            self.start_time.elapsed().as_millis()
        );
    }

    fn test_completed(
        &self,
        test_name: &str,
        file_path: &str,
        passed: bool,
        duration: std::time::Duration,
    ) {
        let mut stdout = self.acquire_stdout();
        if passed {
            tracing::debug!(
                "{} {} in {} at {}us",
                "Passed".green(),
                test_name,
                file_path,
                duration.as_micros()
            );
            let _ = write!(stdout, "{}", ".".green());
        } else {
            tracing::debug!(
                "{} {} in {} at {}us",
                "Failed".red(),
                test_name,
                file_path,
                duration.as_micros()
            );
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
        let total_duration = self.start_time.elapsed();

        let _ = writeln!(stdout);
        let _ = writeln!(stdout, "{}", "─────────────".bold());
        let _ = writeln!(
            stdout,
            "{} {}",
            "Passed tests:".green(),
            stats.passed_tests()
        );
        let _ = writeln!(stdout, "{} {}", "Failed tests:".red(), stats.failed_tests());
        let error_tests = stats.error_tests();
        if error_tests > 0 {
            let _ = writeln!(stdout, "{} {}", "Error tests:".yellow(), error_tests);
        }
        let _ = writeln!(
            stdout,
            "{} {}ms",
            "Total duration:".blue(),
            total_duration.as_millis()
        );
        self.flush_stdout(&mut stdout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Debug)]
    struct SharedBufferWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBufferWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut inner = self.0.lock().unwrap();
            inner.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn strip_ansi_codes(s: &str) -> String {
        let re = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
        re.replace_all(s, "").to_string()
    }

    fn create_test_writer() -> (StdoutDiagnosticWriter, Arc<Mutex<Vec<u8>>>) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let writer = StdoutDiagnosticWriter::new(SharedBufferWriter(buffer.clone()));
        (writer, buffer)
    }

    #[test]
    fn test_test_completed_passed() {
        let (writer, buffer) = create_test_writer();
        writer.test_completed(
            "test_name",
            "test.rs",
            true,
            std::time::Duration::from_micros(100),
        );
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert_eq!(output, ".".green().to_string());
    }

    #[test]
    fn test_test_completed_failed() {
        let (writer, buffer) = create_test_writer();
        writer.test_completed(
            "test_name",
            "test.rs",
            false,
            std::time::Duration::from_micros(100),
        );
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert_eq!(output, ".".red().to_string());
    }

    #[test]
    fn test_test_error() {
        let (writer, buffer) = create_test_writer();
        writer.test_error("test_name", "test.rs", "test error");
        writer.finish(&RunnerResult::new(vec![]));
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert!(output.contains("Error"));
        assert!(output.contains("test_name"));
        assert!(output.contains("test.rs"));
        assert!(output.contains("test error"));
    }

    #[test]
    fn test_discovery() {
        let (writer, buffer) = create_test_writer();
        writer.discovery_started();
        writer.discovery_completed(5);
        writer.finish(&RunnerResult::new(vec![]));
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert!(output.contains("Discovered"));
        assert!(output.contains("5"));
        assert!(output.contains("tests"));
    }

    #[test]
    fn test_finish() {
        let (writer, buffer) = create_test_writer();
        let runner_result = RunnerResult::new(vec![]);
        writer.finish(&runner_result);
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        let output = strip_ansi_codes(&output);
        assert!(output.contains("─────────────"));
        assert!(output.contains("Passed tests: 0"));
        assert!(output.contains("Failed tests: 0"));
        assert!(output.contains("Total duration:"));
    }

    #[test]
    fn test_concurrent_writes() {
        use std::thread;
        let (writer, buffer) = create_test_writer();
        let writer = Arc::new(writer);

        let handles: Vec<_> = (0..10)
            .map(|_| {
                let writer = Arc::clone(&writer);
                thread::spawn(move || {
                    writer.test_completed(
                        "test_name",
                        "test.rs",
                        true,
                        std::time::Duration::from_micros(100),
                    );
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }

        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert_eq!(output.matches(".").count(), 10);
    }
}
