use colored::{Color, Colorize};
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::runner::RunnerResult;
use crate::test_result::{TestResultError, TestResultFail, TestResultPass};

pub trait DiagnosticWriter: Send + Sync {
    /// Called when a test starts running
    fn test_started(&self, test_name: &str, file_path: &str);

    fn test_passed(&self, test: &TestResultPass, duration: std::time::Duration) {
        self.log_test_result(
            "Passed",
            test.test.function_name(),
            test.test.module(),
            duration,
            Color::Green,
            false,
        );
    }

    fn test_failed(&self, test: &TestResultFail, duration: std::time::Duration) {
        self.log_test_result(
            "Failed",
            test.test.function_name(),
            test.test.module(),
            duration,
            Color::Red,
            false,
        );
    }

    fn test_error(&self, test: &TestResultError, duration: std::time::Duration) {
        self.log_test_result(
            "Error",
            test.test.function_name(),
            test.test.module(),
            duration,
            Color::Yellow,
            true,
        );
    }

    fn log_test_result(
        &self,
        status: &str,
        function_name: &str,
        module: &str,
        duration: std::time::Duration,
        color: Color,
        bold: bool,
    ) {
        let mut stdout = self.acquire_stdout();
        let status_text = if bold {
            status.color(color).bold()
        } else {
            status.color(color)
        };

        tracing::info!(
            "{} {} in {} at {}us",
            status_text,
            function_name,
            module,
            duration.as_micros()
        );
        let _ = write!(stdout, "{}", ".".color(color));
        self.flush_stdout(&mut stdout);
    }

    /// Called when test discovery starts
    fn discovery_started(&self);

    /// Called when test discovery completes
    fn discovery_completed(&self, count: usize);

    /// Flush all output to stdout
    fn finish(&self, runner_result: &RunnerResult);

    fn acquire_stdout(&self) -> std::sync::MutexGuard<'_, Box<dyn Write + Send>>;

    fn flush_stdout(&self, stdout: &mut std::sync::MutexGuard<'_, Box<dyn Write + Send>>);
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

    fn maybe_log_test_count(
        &self,
        stdout: &mut std::sync::MutexGuard<'_, Box<dyn Write + Send>>,
        label: &str,
        count: usize,
        color: Color,
    ) {
        if count > 0 {
            let _ = writeln!(
                stdout,
                "{} {}",
                label.color(color),
                count.to_string().color(color)
            );
        }
    }
}

impl DiagnosticWriter for StdoutDiagnosticWriter {
    fn acquire_stdout(&self) -> std::sync::MutexGuard<'_, Box<dyn Write + Send>> {
        self.stdout.lock().unwrap()
    }

    fn flush_stdout(&self, stdout: &mut std::sync::MutexGuard<'_, Box<dyn Write + Send>>) {
        let _ = stdout.flush();
    }

    fn test_started(&self, test_name: &str, file_path: &str) {
        tracing::debug!(
            "{} {} in {} at {}ms",
            "Running".blue(),
            test_name,
            file_path,
            self.start_time.elapsed().as_millis()
        );
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
        if stats.total_tests() > 0 {
            let _ = writeln!(stdout);
            let _ = writeln!(stdout, "{}", "─────────────".bold());
            for (label, num, color) in [
                ("Passed tests:", stats.passed_tests(), Color::Green),
                ("Failed tests:", stats.failed_tests(), Color::Red),
                ("Error tests:", stats.error_tests(), Color::Yellow),
            ] {
                self.maybe_log_test_count(&mut stdout, label, num, color);
            }
            let _ = writeln!(
                stdout,
                "{} {}ms",
                "Total duration:".blue(),
                total_duration.as_millis()
            );
        }
        self.flush_stdout(&mut stdout);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discoverer::DiscoveredTest;
    use crate::runner::RunnerResult;
    use crate::test_result::TestResult;
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

    fn get_discovered_test() -> DiscoveredTest {
        DiscoveredTest::new("test.rs".to_string(), "test_name".to_string())
    }

    #[test]
    fn test_test_passed() {
        let (writer, buffer) = create_test_writer();
        writer.test_passed(
            &TestResultPass {
                test: get_discovered_test(),
            },
            std::time::Duration::from_micros(100),
        );
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert_eq!(output, ".".green().to_string());
    }

    #[test]
    fn test_test_failed() {
        let (writer, buffer) = create_test_writer();
        writer.test_failed(
            &TestResultFail {
                test: get_discovered_test(),
                message: "Test failed".to_string(),
            },
            std::time::Duration::from_micros(100),
        );
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert_eq!(output, ".".red().to_string());
    }

    #[test]
    fn test_test_error() {
        let (writer, buffer) = create_test_writer();
        writer.test_error(
            &TestResultError {
                test: get_discovered_test(),
                message: "Test error".to_string(),
                traceback: "Error traceback".to_string(),
            },
            std::time::Duration::from_micros(100),
        );
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert_eq!(output, ".".yellow().to_string());
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
    fn test_finish_with_mixed_results() {
        let (writer, buffer) = create_test_writer();
        let test_results = vec![
            TestResult::new_pass(get_discovered_test()),
            TestResult::new_fail(get_discovered_test(), "Test failed".to_string()),
            TestResult::new_pass(get_discovered_test()),
        ];
        let runner_result = RunnerResult::new(test_results);
        writer.finish(&runner_result);
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        let output = strip_ansi_codes(&output);
        assert!(output.contains("Passed tests: 2"));
        assert!(output.contains("Failed tests: 1"));
    }

    #[test]
    fn test_finish_with_errors() {
        let (writer, buffer) = create_test_writer();
        let test_results = vec![
            TestResult::new_pass(get_discovered_test()),
            TestResult::new_error(
                get_discovered_test(),
                "Test error message".to_string(),
                "Error traceback".to_string(),
            ),
            TestResult::new_fail(get_discovered_test(), "Test failed".to_string()),
        ];
        let runner_result = RunnerResult::new(test_results);
        writer.finish(&runner_result);
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        let output = strip_ansi_codes(&output);
        assert!(output.contains("Passed tests: 1"));
        assert!(output.contains("Failed tests: 1"));
        assert!(output.contains("Error tests: 1"));
    }

    #[test]
    fn test_finish_with_zero_tests() {
        let (writer, buffer) = create_test_writer();
        let runner_result = RunnerResult::new(vec![]);
        writer.finish(&runner_result);
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        assert_eq!(output, "");
    }

    #[test]
    fn test_finish_with_all_failed() {
        let (writer, buffer) = create_test_writer();
        let test_results = vec![
            TestResult::new_fail(get_discovered_test(), "Test failed".to_string()),
            TestResult::new_fail(get_discovered_test(), "Test failed".to_string()),
        ];
        let runner_result = RunnerResult::new(test_results);
        writer.finish(&runner_result);
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        let output = strip_ansi_codes(&output);
        assert!(output.contains("Failed tests: 2"));
    }

    #[test]
    fn test_finish_with_all_passed() {
        let (writer, buffer) = create_test_writer();
        let test_results = vec![
            TestResult::new_pass(get_discovered_test()),
            TestResult::new_pass(get_discovered_test()),
        ];
        let runner_result = RunnerResult::new(test_results);
        writer.finish(&runner_result);
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        let output = strip_ansi_codes(&output);
        assert!(output.contains("Passed tests: 2"));
    }

    #[test]
    fn test_finish_with_all_error() {
        let (writer, buffer) = create_test_writer();
        let test_results = vec![
            TestResult::new_error(
                get_discovered_test(),
                "Test error message".to_string(),
                "Error traceback".to_string(),
            ),
            TestResult::new_error(
                get_discovered_test(),
                "Test error message".to_string(),
                "Error traceback".to_string(),
            ),
        ];
        let runner_result = RunnerResult::new(test_results);
        writer.finish(&runner_result);
        let output = String::from_utf8(buffer.lock().unwrap().clone()).unwrap();
        let output = strip_ansi_codes(&output);
        assert!(output.contains("Error tests: 2"));
    }
}
