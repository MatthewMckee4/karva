use super::discoverer::DiscoveredTest;

pub struct TestResult {
    discovered_test: DiscoveredTest,
    result: TestResultType,
}

impl TestResult {
    pub fn new(discovered_test: DiscoveredTest, result: TestResultType) -> Self {
        Self {
            discovered_test,
            result,
        }
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
