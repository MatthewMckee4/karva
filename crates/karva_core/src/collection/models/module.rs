use karva_project::Project;
use pyo3::prelude::*;

use crate::{
    collection::TestCase,
    diagnostic::{Diagnostic, reporter::Reporter},
    extensions::fixtures::Finalizers,
    runner::TestRunResult,
};

/// A collected module represents a single Python file with its test cases and finalizers.
#[derive(Default, Debug)]
pub(crate) struct CollectedModule<'proj> {
    /// The test cases in the module.
    test_cases: Vec<TestCase<'proj>>,

    /// Finalizers to run after the tests are executed.
    finalizers: Finalizers,

    /// Fixture diagnostics generated during the collection process.
    fixture_diagnostics: Vec<Diagnostic>,
}

impl<'proj> CollectedModule<'proj> {
    pub(crate) fn total_test_cases(&self) -> usize {
        self.test_cases.len()
    }

    pub(crate) fn add_test_cases(&mut self, test_cases: Vec<TestCase<'proj>>) {
        self.test_cases.extend(test_cases);
    }

    pub(crate) fn add_finalizers(&mut self, finalizers: Finalizers) {
        self.finalizers.update(finalizers);
    }

    pub(crate) fn add_fixture_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        self.fixture_diagnostics.extend(diagnostics);
    }

    pub(crate) fn run(
        self,
        py: Python<'_>,
        project: &Project,
        reporter: &dyn Reporter,
        run_result: &mut TestRunResult,
    ) -> bool {
        let Self {
            test_cases,
            finalizers,
            fixture_diagnostics,
        } = self;

        let mut passed = true;

        let clean_up = |run_result: &mut TestRunResult| {
            run_result.add_test_diagnostics(finalizers.run(py));

            run_result.add_test_diagnostics(fixture_diagnostics);
        };

        for test_case in test_cases {
            passed &= test_case.run(py, reporter, run_result);

            if project.options().fail_fast() && !passed {
                clean_up(run_result);
                return false;
            }
        }

        clean_up(run_result);

        passed
    }
}
