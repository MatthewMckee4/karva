use std::process::{ExitCode, Termination};

#[derive(Copy, Clone)]
/// Stable process exit codes distinguishing test failures from runner errors.
pub enum ExitStatus {
    /// Checking was successful and there were no errors.
    Success = 0,

    /// Checking was successful but there were errors.
    Failure = 1,

    /// Checking failed.
    Error = 2,
}

impl Termination for ExitStatus {
    fn report(self) -> ExitCode {
        ExitCode::from(self as u8)
    }
}

impl ExitStatus {
    /// Returns the numeric process exit code.
    pub fn to_i32(self) -> i32 {
        self as i32
    }
}
