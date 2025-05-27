use super::discoverer::DiscoveredTest;

#[derive(Debug, Clone)]
pub struct TestResultPass {
    pub test: DiscoveredTest,
}

#[derive(Debug, Clone)]
pub struct TestResultFail {
    pub test: DiscoveredTest,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct TestResultError {
    pub test: DiscoveredTest,
    pub message: String,
    pub traceback: String,
}

#[derive(Debug, Clone)]
pub enum TestResult {
    Pass(TestResultPass),
    Fail(TestResultFail),
    Error(TestResultError),
}

impl TestResult {
    pub fn new_pass(test: DiscoveredTest) -> Self {
        Self::Pass(TestResultPass { test })
    }

    pub fn new_fail(test: DiscoveredTest, message: String) -> Self {
        Self::Fail(TestResultFail { test, message })
    }

    pub fn new_error(test: DiscoveredTest, message: String, traceback: String) -> Self {
        Self::Error(TestResultError {
            test,
            message,
            traceback,
        })
    }

    pub fn test(&self) -> &DiscoveredTest {
        match self {
            Self::Pass(TestResultPass { test }) => test,
            Self::Fail(TestResultFail { test, .. }) => test,
            Self::Error(TestResultError { test, .. }) => test,
        }
    }

    pub fn is_pass(&self) -> bool {
        matches!(self, Self::Pass(_))
    }

    pub fn is_fail(&self) -> bool {
        matches!(self, Self::Fail(_))
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Self::Error(_))
    }

    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Pass(_) => None,
            Self::Fail(TestResultFail { message, .. }) => Some(message),
            Self::Error(TestResultError { message, .. }) => Some(message),
        }
    }

    pub fn traceback(&self) -> Option<&str> {
        match self {
            Self::Pass(_) | Self::Fail(_) => None,
            Self::Error(TestResultError { traceback, .. }) => Some(traceback),
        }
    }
}
