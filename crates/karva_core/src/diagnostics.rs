use colored::Colorize;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

pub trait DiagnosticWriter: Send + Sync {
    /// Called when a test starts running
    fn test_started(&mut self, test_name: &str, file_path: &str) -> io::Result<()>;

    /// Called when a test completes
    fn test_completed(&mut self, test_name: &str, file_path: &str, passed: bool) -> io::Result<()>;

    /// Called when a test fails with an error message
    fn test_error(&mut self, test_name: &str, file_path: &str, error: &str) -> io::Result<()>;

    /// Called when test discovery starts
    fn discovery_started(&mut self) -> io::Result<()>;

    /// Called when test discovery completes
    fn discovery_completed(&mut self, count: usize) -> io::Result<()>;

    /// Flush all output to stdout
    fn flush(&mut self) -> io::Result<()>;
}

pub struct StdoutDiagnosticWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl StdoutDiagnosticWriter {
    pub fn new(buffer: Vec<u8>) -> Self {
        Self {
            buffer: Arc::new(Mutex::new(buffer)),
        }
    }
}

impl DiagnosticWriter for StdoutDiagnosticWriter {
    fn test_started(&mut self, test_name: &str, file_path: &str) -> io::Result<()> {
        tracing::debug!("{} {} in {}", "Running".blue(), test_name, file_path);
        Ok(())
    }

    fn test_completed(&mut self, test_name: &str, file_path: &str, passed: bool) -> io::Result<()> {
        let mut buffer = self.buffer.lock().unwrap();
        if passed {
            tracing::debug!("{} {} in {}", "Passed".green(), test_name, file_path);
            write!(buffer, "{}", ".".green())
        } else {
            tracing::debug!("{} {} in {}", "Failed".red(), test_name, file_path);
            write!(buffer, "{}", ".".red())
        }
    }

    fn test_error(&mut self, test_name: &str, file_path: &str, error: &str) -> io::Result<()> {
        let mut buffer = self.buffer.lock().unwrap();
        writeln!(
            buffer,
            "{} {} in {}: {}",
            "Error".red().bold(),
            test_name,
            file_path,
            error
        )
    }

    fn discovery_started(&mut self) -> io::Result<()> {
        tracing::debug!("{}", "Discovering tests...".blue());
        Ok(())
    }

    fn discovery_completed(&mut self, count: usize) -> io::Result<()> {
        let mut buffer = self.buffer.lock().unwrap();
        writeln!(
            buffer,
            "{} {} {}",
            "Discovered".blue(),
            count,
            "tests".blue()
        )
    }

    fn flush(&mut self) -> io::Result<()> {
        let buffer = self.buffer.lock().unwrap();
        io::stdout().write_all(&buffer)?;
        io::stdout().flush()
    }
}
