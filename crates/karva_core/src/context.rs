use std::sync::{Arc, Mutex};

use karva_project::Project;

use crate::{
    Reporter, TestRunResult,
    diagnostic::{DiagnosticGuardBuilder, DiagnosticType},
};

pub struct Context<'proj, 'rep> {
    project: &'proj Project,
    result: Arc<Mutex<TestRunResult>>,
    reporter: &'rep dyn Reporter,
}

impl<'proj, 'rep> Context<'proj, 'rep> {
    pub fn new(project: &'proj Project, reporter: &'rep dyn Reporter) -> Self {
        Self {
            project,
            result: Arc::new(Mutex::new(TestRunResult::default())),
            reporter,
        }
    }

    pub const fn project(&self) -> &'proj Project {
        self.project
    }

    pub fn result(&self) -> std::sync::MutexGuard<'_, TestRunResult> {
        self.result.lock().unwrap()
    }

    pub fn reporter(&self) -> &'rep dyn Reporter {
        self.reporter
    }

    pub(crate) fn into_result(self) -> TestRunResult {
        self.result.lock().unwrap().clone().into_sorted()
    }

    pub(crate) fn report_diagnostic<'ctx>(
        &'ctx self,
        rule: &'static DiagnosticType,
    ) -> DiagnosticGuardBuilder<'ctx, 'proj, 'rep> {
        DiagnosticGuardBuilder::new(self, rule)
    }
}
