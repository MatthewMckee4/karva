use std::sync::{Arc, Mutex};

use karva_collector::CollectionSettings;
use karva_diagnostic::{IndividualTestResultKind, Reporter, TestRunResult};
use karva_metadata::ProjectSettings;
use karva_python_semantic::QualifiedTestName;
use karva_system::System;
use ruff_python_ast::PythonVersion;

use crate::diagnostic::{DiagnosticGuardBuilder, DiagnosticType};

pub struct Context<'a> {
    system: &'a dyn System,
    settings: &'a ProjectSettings,
    python_version: PythonVersion,
    result: Arc<Mutex<TestRunResult>>,
    reporter: &'a dyn Reporter,
}

impl<'a> Context<'a> {
    pub(crate) fn new(
        system: &'a dyn System,
        settings: &'a ProjectSettings,
        python_version: PythonVersion,
        reporter: &'a dyn Reporter,
    ) -> Self {
        Self {
            system,
            settings,
            python_version,
            result: Arc::new(Mutex::new(TestRunResult::default())),
            reporter,
        }
    }

    pub(crate) fn system(&self) -> &'a dyn System {
        self.system
    }

    pub(crate) fn settings(&self) -> &'a ProjectSettings {
        self.settings
    }

    pub(crate) fn collection_settings(&'a self) -> CollectionSettings<'a> {
        CollectionSettings {
            python_version: self.python_version,
            test_function_prefix: &self.settings.test().test_function_prefix,
            respect_ignore_files: self.settings.src().respect_ignore_files,
        }
    }

    pub(crate) fn result(&self) -> std::sync::MutexGuard<'_, TestRunResult> {
        self.result.lock().unwrap()
    }

    pub(crate) fn into_result(self) -> TestRunResult {
        self.result.lock().unwrap().clone().into_sorted()
    }

    pub fn register_test_case_result(
        &self,
        test_case_name: &QualifiedTestName,
        test_result: IndividualTestResultKind,
        duration: std::time::Duration,
    ) -> bool {
        let result = match &test_result {
            IndividualTestResultKind::Passed | IndividualTestResultKind::Skipped { .. } => true,
            IndividualTestResultKind::Failed => false,
        };

        self.result().register_test_case_result(
            test_case_name,
            test_result,
            duration,
            Some(self.reporter),
        );

        result
    }

    pub(crate) const fn report_diagnostic<'ctx>(
        &'ctx self,
        rule: &'static DiagnosticType,
    ) -> DiagnosticGuardBuilder<'ctx, 'a> {
        DiagnosticGuardBuilder::new(self, rule, false)
    }

    pub(crate) const fn report_discovery_diagnostic<'ctx>(
        &'ctx self,
        rule: &'static DiagnosticType,
    ) -> DiagnosticGuardBuilder<'ctx, 'a> {
        DiagnosticGuardBuilder::new(self, rule, true)
    }
}
