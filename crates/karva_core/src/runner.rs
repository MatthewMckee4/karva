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
            let file_path = test.path().as_str();

            self.diagnostics
                .test_started(test_name, file_path)
                .unwrap_or_default();
            let test_result = self.run_test(&test);

            match test_result {
                Ok(test_result) => {
                    let passed = test_result.result() == &TestResultType::Pass;
                    self.diagnostics
                        .test_completed(test_name, file_path, passed)
                        .unwrap_or_default();
                    test_results.push(test_result);
                }
                Err(e) => {
                    self.diagnostics
                        .test_error(test_name, file_path, &e.to_string())
                        .unwrap_or_default();
                }
            }
        }

        RunnerResult::new(test_results)
    }

    fn run_test(&self, test: &DiscoveredTest) -> PyResult<TestResult> {
        Python::with_gil(|py| {
            let sys_path = py.import("sys")?;
            let path = sys_path.getattr("path")?;
            path.call_method1("append", (self.project.cwd().as_str(),))?;

            if let Some(file_name) = test.path().file_name() {
                let module_name = file_name.replace(".py", "");

                let my_module = PyModule::import(py, module_name)?;

                let function = my_module.getattr(test.function_name())?;

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
            } else {
                Ok(TestResult::new(test.clone(), TestResultType::Fail))
            }
        })
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
