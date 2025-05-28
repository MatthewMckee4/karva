use super::discovery::DiscoveredTest;

#[derive(Debug, Clone)]
pub struct TestResultPass {
    pub test: DiscoveredTest,
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct TestResultFail {
    pub test: DiscoveredTest,
    pub traceback: Option<String>,
    pub duration: std::time::Duration,
}

#[derive(Debug, Clone)]
pub struct TestResultError {
    pub test: DiscoveredTest,
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
    pub fn new_pass(test: DiscoveredTest, duration: std::time::Duration) -> Self {
        Self::Pass(TestResultPass { test, duration })
    }

    pub fn new_fail(
        test: DiscoveredTest,
        traceback: Option<String>,
        duration: std::time::Duration,
    ) -> Self {
        Self::Fail(TestResultFail {
            test,
            traceback,
            duration,
        })
    }

    pub fn new_error(
        test: DiscoveredTest,
        traceback: String,
        duration: std::time::Duration,
    ) -> Self {
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
