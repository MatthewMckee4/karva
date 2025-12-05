use std::io::StdoutLock;

use karva_project::VerbosityLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Printer {
    verbosity: VerbosityLevel,
    no_progress: bool,
}

impl Printer {
    pub const fn new(verbosity: VerbosityLevel, no_progress: bool) -> Self {
        Self {
            verbosity,
            no_progress,
        }
    }

    /// Return the [`Stdout`] stream for important messages.
    ///
    /// Unlike [`Self::stdout_general`], the returned stream will be enabled when
    /// [`VerbosityLevel::Quiet`] is used.
    const fn stdout_important(self) -> Stdout {
        match self.verbosity {
            VerbosityLevel::Silent => Stdout::disabled(),
            VerbosityLevel::Quiet
            | VerbosityLevel::Default
            | VerbosityLevel::Verbose
            | VerbosityLevel::ExtraVerbose
            | VerbosityLevel::Trace => Stdout::enabled(),
        }
    }

    /// Return the [`Stdout`] stream for general messages.
    ///
    /// The returned stream will be disabled when [`VerbosityLevel::Quiet`] is used.
    const fn stdout_general(self) -> Stdout {
        match self.verbosity {
            VerbosityLevel::Silent | VerbosityLevel::Quiet => Stdout::disabled(),
            VerbosityLevel::Default
            | VerbosityLevel::Verbose
            | VerbosityLevel::ExtraVerbose
            | VerbosityLevel::Trace => Stdout::enabled(),
        }
    }

    /// Return the [`Stdout`] stream for a summary message that was explicitly requested by the
    /// user.
    pub const fn stream_for_requested_summary(self) -> Stdout {
        self.stdout_important()
    }

    /// Return the [`Stdout`] stream for a summary message on failure.
    pub const fn stream_for_failure_summary(self) -> Stdout {
        self.stdout_important()
    }

    /// Return the [`Stdout`] stream for a summary message on success.
    pub const fn stream_for_success_summary(self) -> Stdout {
        self.stdout_general()
    }

    /// Return the [`Stdout`] stream for detailed messages.
    pub const fn stream_for_details(self) -> Stdout {
        self.stdout_general()
    }

    pub(crate) const fn stream_for_test_result(self) -> Stdout {
        if self.no_progress {
            Stdout::disabled()
        } else {
            self.stdout_general()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamStatus {
    Enabled,
    Disabled,
}

#[derive(Debug)]
pub struct Stdout {
    status: StreamStatus,
    lock: Option<StdoutLock<'static>>,
}

impl Stdout {
    const fn enabled() -> Self {
        Self {
            status: StreamStatus::Enabled,
            lock: None,
        }
    }

    const fn disabled() -> Self {
        Self {
            status: StreamStatus::Disabled,
            lock: None,
        }
    }

    pub fn lock(mut self) -> Self {
        match self.status {
            StreamStatus::Enabled => {
                self.lock.take();
                self.lock = Some(std::io::stdout().lock());
            }
            StreamStatus::Disabled => self.lock = None,
        }
        self
    }

    fn handle(&mut self) -> Box<dyn std::io::Write + '_> {
        match self.lock.as_mut() {
            Some(lock) => Box::new(lock),
            None => Box::new(std::io::stdout()),
        }
    }

    pub const fn is_enabled(&self) -> bool {
        matches!(self.status, StreamStatus::Enabled)
    }
}

impl std::fmt::Write for Stdout {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        match self.status {
            StreamStatus::Enabled => {
                let _ = write!(self.handle(), "{s}");
                Ok(())
            }
            StreamStatus::Disabled => Ok(()),
        }
    }
}
