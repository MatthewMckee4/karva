use std::sync::{Arc, Mutex};

use karva_project::{Db, Project};

use crate::{
    IndividualTestResultKind, Reporter, TestRunResult,
    diagnostic::{DiagnosticGuardBuilder, DiagnosticType},
};

pub struct Context<'db, 'rep> {
    db: &'db dyn Db,
    result: Arc<Mutex<TestRunResult>>,
    reporter: &'rep dyn Reporter,
}

impl<'db, 'rep> Context<'db, 'rep> {
    pub(crate) fn new(db: &'db dyn Db, reporter: &'rep dyn Reporter) -> Self {
        Self {
            db,
            result: Arc::new(Mutex::new(TestRunResult::default())),
            reporter,
        }
    }

    pub(crate) fn db<'a>(&'a self) -> &'db (dyn Db + 'a) {
        self.db
    }

    pub(crate) fn project(&self) -> &Project {
        self.db.project()
    }

    pub(crate) fn result(&self) -> std::sync::MutexGuard<'_, TestRunResult> {
        self.result.lock().unwrap()
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
    ) -> DiagnosticGuardBuilder<'ctx, 'db, 'rep> {
        DiagnosticGuardBuilder::new(self, rule, false)
    }

    pub(crate) const fn report_discovery_diagnostic<'ctx>(
        &'ctx self,
        rule: &'static DiagnosticType,
    ) -> DiagnosticGuardBuilder<'ctx, 'db, 'rep> {
        DiagnosticGuardBuilder::new(self, rule, true)
    }
}
