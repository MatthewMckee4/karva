use crate::discovery::TestCase;

#[derive(Debug, Clone)]
pub struct Pass<'proj> {
    pub test: TestCase<'proj>,
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct Fail<'proj> {
    pub test: TestCase<'proj>,
    pub traceback: Option<String>,
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct Error<'proj> {
    pub test: TestCase<'proj>,
    pub traceback: String,
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone)]
pub enum TestResult<'proj> {
    Pass(Pass<'proj>),
    Fail(Fail<'proj>),
    Error(Error<'proj>),
}

impl<'proj> TestResult<'proj> {
    #[must_use]
    pub const fn new_pass(test: TestCase<'proj>, duration: std::time::Duration) -> Self {
        Self::Pass(Pass { test, duration })
    }

    #[must_use]
    pub const fn new_fail(
        test: TestCase<'proj>,
        traceback: Option<String>,
        duration: std::time::Duration,
    ) -> Self {
        Self::Fail(Fail {
            test,
            traceback,
            duration,
        })
    }

    #[must_use]
    pub const fn new_error(
        test: TestCase<'proj>,
        traceback: String,
        duration: std::time::Duration,
    ) -> Self {
        Self::Error(Error {
            test,
            traceback,
            duration,
        })
    }

    #[must_use]
    pub const fn is_success(&self) -> bool {
        matches!(self, Self::Pass(_))
    }
}
