use super::discovery::TestCase;

#[derive(Debug, Clone)]
pub struct TestResultPass {
    pub test: TestCase,
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct TestResultFail {
    pub test: TestCase,
    pub traceback: Option<String>,
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct TestResultError {
    pub test: TestCase,
    pub traceback: String,
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone)]
pub enum TestResult {
    Pass(TestResultPass),
    Fail(TestResultFail),
    Error(TestResultError),
}

impl TestResult {
    pub fn new_pass(test: TestCase, duration: std::time::Duration) -> Self {
        Self::Pass(TestResultPass { test, duration })
    }

    pub fn new_fail(
        test: TestCase,
        traceback: Option<String>,
        duration: std::time::Duration,
    ) -> Self {
        Self::Fail(TestResultFail {
            test,
            traceback,
            duration,
        })
    }

    pub fn new_error(test: TestCase, traceback: String, duration: std::time::Duration) -> Self {
        Self::Error(TestResultError {
            test,
            traceback,
            duration,
        })
    }

    pub fn is_pass(&self) -> bool {
        matches!(self, Self::Pass(_))
    }
}
