use karva_project::Project;

use crate::{Reporter, TestRunResult};

pub struct Context<'proj, 'rep> {
    project: &'proj Project,
    result: TestRunResult,
    reporter: &'rep dyn Reporter,
}

impl<'proj, 'rep> Context<'proj, 'rep> {
    pub fn new(project: &'proj Project, reporter: &'rep dyn Reporter) -> Self {
        Self {
            project,
            result: TestRunResult::default(),
            reporter,
        }
    }

    pub const fn project(&self) -> &'proj Project {
        self.project
    }

    pub const fn result_mut(&mut self) -> &mut TestRunResult {
        &mut self.result
    }

    pub fn reporter(&self) -> &'rep dyn Reporter {
        self.reporter
    }

    pub(crate) fn into_result(self) -> TestRunResult {
        self.result
    }
}
