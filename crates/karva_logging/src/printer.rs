use std::io::{self, StdoutLock, Write};

use crate::status_level::{FinalStatusLevel, StatusLevel};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Printer {
    status_level: StatusLevel,
    final_status_level: FinalStatusLevel,
}

impl Printer {
    pub fn new(status_level: StatusLevel, final_status_level: FinalStatusLevel) -> Self {
        Self {
            status_level,
            final_status_level,
        }
    }

    pub fn status_level(self) -> StatusLevel {
        self.status_level
    }

    pub fn final_status_level(self) -> FinalStatusLevel {
        self.final_status_level
    }

    /// Stream for the "Starting N tests" header and per-test result lines.
    ///
    /// The reporter additionally filters individual results by [`StatusLevel`].
    pub fn stream_for_test_result(self) -> Stdout {
        Stdout::new(self.status_level != StatusLevel::None)
    }

    /// Stream for the end-of-run summary line.
    ///
    /// `success` is true when no tests failed. `had_retries` is true when at
    /// least one test was retried; it elevates `final-status-level=retry` (or
    /// higher) to show the summary even when all tests eventually passed.
    pub fn stream_for_summary(self, success: bool, had_retries: bool) -> Stdout {
        let enabled = match self.final_status_level {
            FinalStatusLevel::None => false,
            FinalStatusLevel::Fail => !success,
            FinalStatusLevel::Retry | FinalStatusLevel::Slow => !success || had_retries,
            FinalStatusLevel::Pass | FinalStatusLevel::Skip | FinalStatusLevel::All => true,
        };
        Stdout::new(enabled)
    }

    /// Stream for the diagnostic block (tracebacks, durations) at the end of the run.
    pub fn stream_for_details(self) -> Stdout {
        Stdout::new(self.final_status_level != FinalStatusLevel::None)
    }

    /// Stream for messages explicitly requested by the user, such as
    /// `warning: no tests to run`. Suppressed only when both status levels are `none`.
    pub fn stream_for_message(self) -> Stdout {
        let both_none = self.status_level == StatusLevel::None
            && self.final_status_level == FinalStatusLevel::None;
        Stdout::new(!both_none)
    }
}

#[derive(Debug)]
pub struct Stdout {
    enabled: bool,
    lock: Option<StdoutLock<'static>>,
}

impl Stdout {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            lock: None,
        }
    }

    #[must_use]
    pub fn lock(mut self) -> Self {
        if self.enabled {
            self.lock = Some(std::io::stdout().lock());
        }
        self
    }

    fn handle(&mut self) -> Box<dyn Write + '_> {
        if let Some(lock) = self.lock.as_mut() {
            Box::new(lock)
        } else {
            Box::new(std::io::stdout())
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}

impl Write for Stdout {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.enabled {
            self.handle().write(buf)
        } else {
            Ok(buf.len())
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.enabled {
            self.handle().flush()
        } else {
            Ok(())
        }
    }
}

impl From<Stdout> for std::process::Stdio {
    fn from(val: Stdout) -> Self {
        if val.enabled {
            Self::inherit()
        } else {
            Self::null()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::Stdout;

    #[test]
    fn disabled_stdout_accepts_writes() {
        let mut stdout = Stdout::new(false).lock();

        stdout
            .write_all(b"suppressed output")
            .expect("disabled stdout should discard writes");
        stdout
            .flush()
            .expect("disabled stdout should flush as a no-op");
    }
}
