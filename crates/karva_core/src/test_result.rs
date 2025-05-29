use super::discovery::TestCase;

#[derive(Debug, Clone)]
pub struct Pass {
    pub test: TestCase,
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct Fail {
    pub test: TestCase,
    pub traceback: Option<String>,
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct Error {
    pub test: TestCase,
    pub traceback: String,
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone)]
pub enum TestResult {
    Pass(Pass),
    Fail(Fail),
    Error(Error),
}

impl TestResult {
    #[must_use]
    pub const fn new_pass(test: TestCase, duration: std::time::Duration) -> Self {
        Self::Pass(Pass { test, duration })
    }

    #[must_use]
    pub const fn new_fail(
        test: TestCase,
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
        test: TestCase,
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
    pub const fn is_pass(&self) -> bool {
        matches!(self, Self::Pass(_))
    }
}
