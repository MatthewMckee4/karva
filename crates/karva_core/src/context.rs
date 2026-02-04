use std::cell::RefCell;
use std::rc::Rc;

use karva_collector::CollectionSettings;
use karva_diagnostic::{IndividualTestResultKind, Reporter, TestRunResult};
use karva_metadata::ProjectSettings;
use karva_python_semantic::QualifiedTestName;
use karva_system::System;
use ruff_python_ast::PythonVersion;

use crate::diagnostic::{DiagnosticGuardBuilder, DiagnosticType};

/// Central context object that holds shared state for a test run.
///
/// Provides access to system operations, project settings, and test result
/// accumulation throughout the test discovery and execution phases.
pub struct Context<'a> {
    /// Reference to the system abstraction for file operations.
    system: &'a dyn System,

    /// Project-level configuration settings.
    settings: &'a ProjectSettings,

    /// The Python version being used for this test run.
    python_version: PythonVersion,

    /// Accumulated test results, wrapped in `RefCell` for interior mutability.
    result: Rc<RefCell<TestRunResult>>,

    /// Reporter for outputting test progress and results.
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
            result: Rc::new(RefCell::new(TestRunResult::default())),
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
            collect_fixtures: true,
        }
    }

    pub(crate) fn result(&self) -> std::cell::RefMut<'_, TestRunResult> {
        self.result.borrow_mut()
    }

    pub(crate) fn into_result(self) -> TestRunResult {
        self.result.borrow().clone().into_sorted()
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

    pub fn python_version(&self) -> PythonVersion {
        self.python_version
    }
}
