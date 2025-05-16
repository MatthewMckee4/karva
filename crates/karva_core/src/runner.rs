use crate::{
    project::Project,
    test_result::{TestResult, TestResultType},
};

pub struct Runner {
    project: Project,
}

impl Runner {
    pub fn new(project: Project) -> Self {
        Self { project }
    }

    pub fn run(&self) -> RunnerResult {
        RunnerResult::new(vec![])
    }
}

pub struct RunnerResult {
    test_results: Vec<TestResult>,
}

impl RunnerResult {
    pub fn new(test_results: Vec<TestResult>) -> Self {
        Self { test_results }
    }

    pub fn passed(&self) -> bool {
        self.test_results
            .iter()
            .all(|test_result| test_result.result() == &TestResultType::Pass)
    }
}
