use pyo3::{exceptions::PyAssertionError, prelude::*};

use crate::{
    diagnostics::DiagnosticWriter,
    discoverer::{DiscoveredTest, Discoverer},
    project::Project,
    test_result::{TestResult, TestResultType},
};

pub struct Runner<'a> {
    project: &'a Project,
    diagnostics: Box<dyn DiagnosticWriter>,
}

impl<'a> Runner<'a> {
    pub fn new(project: &'a Project, diagnostics: Box<dyn DiagnosticWriter>) -> Self {
        Self {
            project,
            diagnostics,
        }
    }

    pub fn diagnostics(&mut self) -> &mut dyn DiagnosticWriter {
        &mut *self.diagnostics
    }

    pub fn run(&mut self) -> RunnerResult {
        self.diagnostics.discovery_started().unwrap_or_default();
        let discovered_tests = Discoverer::new(self.project).discover();
        self.diagnostics
            .discovery_completed(discovered_tests.len())
            .unwrap_or_default();

        let mut test_results = Vec::new();
        for test in discovered_tests {
            let test_name = test.function_name().as_str();
            let module = test.module();

            self.diagnostics
                .test_started(test_name, module)
                .unwrap_or_default();

            let test_result = self.run_test(&test);

            match test_result {
                Ok(test_result) => {
                    let passed = test_result.result() == &TestResultType::Pass;
                    self.diagnostics
                        .test_completed(test_name, module, passed)
                        .unwrap_or_default();
                    test_results.push(test_result);
                }
                Err(e) => {
                    self.diagnostics
                        .test_error(test_name, module, &e.to_string())
                        .unwrap_or_default();
                    test_results.push(TestResult::new(test.clone(), TestResultType::Error));
                }
            }
        }

        RunnerResult::new(test_results)
    }

    fn run_test(&self, test: &DiscoveredTest) -> PyResult<TestResult> {
        Python::with_gil(|py| {
            self.add_cwd_to_sys_path(&py)?;

            let imported_module = PyModule::import(py, test.module())?;

            let function = imported_module.getattr(test.function_name())?;

            let result = function.call((), None);

            match result {
                Ok(_) => Ok(TestResult::new(test.clone(), TestResultType::Pass)),
                Err(err) => {
                    let err_value = err.value(py);
                    if err_value.is_instance_of::<PyAssertionError>() {
                        Ok(TestResult::new(test.clone(), TestResultType::Fail))
                    } else {
                        Err(err)
                    }
                }
            }
        })
    }

    fn add_cwd_to_sys_path(&self, py: &Python) -> PyResult<()> {
        let sys_path = py.import("sys")?;
        let path = sys_path.getattr("path")?;
        path.call_method1("append", (self.project.cwd().as_str(),))?;
        Ok(())
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
