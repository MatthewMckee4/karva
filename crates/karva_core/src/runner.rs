use pyo3::{exceptions::PyAssertionError, prelude::*};
use std::time::Instant;

use crate::{
    diagnostics::DiagnosticWriter,
    discoverer::{DiscoveredTest, Discoverer},
    project::Project,
    test_result::{TestResult, TestResultError},
};

pub struct Runner<'a> {
    project: &'a Project,
    diagnostic_writer: Box<dyn DiagnosticWriter>,
}

impl<'a> Runner<'a> {
    pub fn new(project: &'a Project, diagnostics: Box<dyn DiagnosticWriter>) -> Self {
        Self {
            project,
            diagnostic_writer: diagnostics,
        }
    }

    pub fn diagnostic_writer(&self) -> &dyn DiagnosticWriter {
        &*self.diagnostic_writer
    }

    pub fn run(&mut self) -> RunnerResult {
        self.diagnostic_writer.discovery_started();
        let discovered_tests = Discoverer::new(self.project).discover();
        self.diagnostic_writer
            .discovery_completed(discovered_tests.len());

        let test_results = Python::with_gil(|py| {
            let add_cwd_to_sys_path_result = self.add_cwd_to_sys_path(&py);

            if add_cwd_to_sys_path_result.is_err() {
                return Err("Failed to add cwd to sys.path".to_string());
            }
            Ok(discovered_tests
                .iter()
                .map(|test| {
                    let test_name = test.function_name().as_str();
                    let module = test.module();

                    self.diagnostic_writer.test_started(test_name, module);

                    let start_time = Instant::now();
                    let test_result = self.run_test(&py, test);
                    let duration = start_time.elapsed();

                    match test_result {
                        Ok(test_result) => {
                            match &test_result {
                                TestResult::Pass(test_result) => {
                                    self.diagnostic_writer.test_passed(test_result, duration);
                                }
                                TestResult::Fail(test_result) => {
                                    self.diagnostic_writer.test_failed(test_result, duration);
                                }
                                TestResult::Error(test_result) => {
                                    self.diagnostic_writer.test_error(test_result, duration);
                                }
                            }
                            test_result
                        }
                        Err(error_msg) => {
                            let test_result = TestResultError {
                                test: test.clone(),
                                message: error_msg.clone(),
                                traceback: error_msg.clone(),
                            };
                            self.diagnostic_writer.test_error(&test_result, duration);
                            TestResult::Error(test_result)
                        }
                    }
                })
                .collect())
        })
        .unwrap_or_default();

        let runner_result = RunnerResult::new(test_results);
        self.diagnostic_writer.finish(&runner_result);
        runner_result
    }

    fn run_test(&self, py: &Python, test: &DiscoveredTest) -> Result<TestResult, String> {
        let imported_module = PyModule::import(*py, test.module()).map_err(|e| e.to_string())?;
        let function = imported_module
            .getattr(test.function_name())
            .map_err(|e| e.to_string())?;

        match function.call((), None) {
            Ok(_) => Ok(TestResult::new_pass(test.clone())),
            Err(err) => {
                let err_value = err.value(*py);
                if err_value.is_instance_of::<PyAssertionError>() {
                    let message = err_value.to_string();
                    Ok(TestResult::new_fail(test.clone(), message))
                } else {
                    let message = err_value.to_string();
                    let traceback = err
                        .traceback(*py)
                        .map(|tb| tb.format().unwrap_or_default())
                        .unwrap_or_default();
                    Ok(TestResult::new_error(test.clone(), message, traceback))
                }
            }
        }
    }

    fn add_cwd_to_sys_path(&self, py: &Python) -> PyResult<()> {
        let sys_path = py.import("sys")?;
        let path = sys_path.getattr("path")?;
        path.call_method1("append", (self.project.cwd().as_str(),))?;
        Ok(())
    }
}

#[derive(Debug)]
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
            .all(|test_result| test_result.is_pass())
    }

    pub fn test_results(&self) -> &[TestResult] {
        &self.test_results
    }

    pub fn stats(&self) -> RunnerStats {
        let mut stats = RunnerStats::default();
        for test_result in &self.test_results {
            stats.total_tests += 1;
            match test_result {
                TestResult::Pass(_) => stats.passed_tests += 1,
                TestResult::Fail(_) => stats.failed_tests += 1,
                TestResult::Error(_) => stats.error_tests += 1,
            }
        }
        stats
    }
}

#[derive(Debug, Default)]
pub struct RunnerStats {
    total_tests: usize,
    passed_tests: usize,
    failed_tests: usize,
    error_tests: usize,
}

impl RunnerStats {
    pub fn total_tests(&self) -> usize {
        self.total_tests
    }

    pub fn passed_tests(&self) -> usize {
        self.passed_tests
    }

    pub fn failed_tests(&self) -> usize {
        self.failed_tests
    }

    pub fn error_tests(&self) -> usize {
        self.error_tests
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::Project;
    use std::sync::{Arc, Mutex};
    use std::{io::Write, path::PathBuf};

    struct MockDiagnostics {
        stdout: Arc<Mutex<Box<dyn Write + Send>>>,
    }

    impl DiagnosticWriter for MockDiagnostics {
        fn discovery_started(&self) {}

        fn discovery_completed(&self, _count: usize) {}

        fn test_started(&self, _name: &str, _module: &str) {}

        fn finish(&self, _runner_result: &RunnerResult) {}

        fn acquire_stdout(&self) -> std::sync::MutexGuard<'_, Box<dyn Write + Send>> {
            self.stdout.lock().unwrap()
        }

        fn flush_stdout(&self, stdout: &mut std::sync::MutexGuard<'_, Box<dyn Write + Send>>) {
            let _ = stdout.flush();
        }
    }

    #[test]
    fn test_runner_result_passed() {
        let project = Project::new(PathBuf::from(".").into(), vec![], "test".to_string());
        let diagnostics = Box::new(MockDiagnostics {
            stdout: Arc::new(Mutex::new(Box::new(Vec::new()))),
        });
        let mut runner = Runner::new(&project, diagnostics);
        let result = runner.run();
        assert!(result.passed());
    }
}
