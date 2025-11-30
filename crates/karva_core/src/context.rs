use std::sync::{Arc, Mutex};

use karva_project::Project;

use crate::{
    IndividualTestResultKind, Reporter, TestRunResult,
    diagnostic::{DiagnosticGuardBuilder, DiagnosticType, TestRunResultDisplayOptions},
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
            result: Arc::new(Mutex::new(TestRunResult::new(
                project.cwd().as_std_path().to_path_buf(),
                TestRunResultDisplayOptions {
                    diagnostic_format: project.options().diagnostic_format(),
                },
            ))),
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

    pub fn register_test_case_result(
        &self,
        test_case_name: &str,
        test_result: IndividualTestResultKind,
    ) -> bool {
        let result = match &test_result {
            IndividualTestResultKind::Passed | IndividualTestResultKind::Skipped { .. } => true,
            IndividualTestResultKind::Failed => false,
        };

        self.result()
            .register_test_case_result(test_case_name, test_result, Some(self.reporter));

        result
    }

    pub(crate) const fn report_diagnostic<'ctx>(
        &'ctx self,
        rule: &'static DiagnosticType,
    ) -> DiagnosticGuardBuilder<'ctx, 'proj, 'rep> {
        DiagnosticGuardBuilder::new(self, rule, false)
    }

    pub(crate) const fn report_discovery_diagnostic<'ctx>(
        &'ctx self,
        rule: &'static DiagnosticType,
    ) -> DiagnosticGuardBuilder<'ctx, 'proj, 'rep> {
        DiagnosticGuardBuilder::new(self, rule, true)
    }
}
