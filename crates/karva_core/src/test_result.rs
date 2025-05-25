use super::discoverer::DiscoveredTest;

pub struct TestResult {
    test: DiscoveredTest,
    result: TestResultType,
}

impl TestResult {
    pub fn new(test: DiscoveredTest, result: TestResultType) -> Self {
        Self { test, result }
    }

    pub fn test(&self) -> &DiscoveredTest {
        &self.test
    }

    pub fn result(&self) -> &TestResultType {
        &self.result
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum TestResultType {
    Pass,
    Fail,
}
