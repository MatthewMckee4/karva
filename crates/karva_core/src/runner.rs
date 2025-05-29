use pyo3::prelude::*;

use crate::{
    diagnostics::DiagnosticWriter, discovery::Discoverer, project::Project, test_result::TestResult,
};

pub struct Runner<'a> {
    project: &'a Project,
    diagnostic_writer: DiagnosticWriter,
}

impl<'a> Runner<'a> {
    pub const fn new(project: &'a Project, diagnostics: DiagnosticWriter) -> Self {
        Self {
            project,
            diagnostic_writer: diagnostics,
        }
    }

    pub const fn diagnostic_writer(&self) -> &DiagnosticWriter {
        &self.diagnostic_writer
    }

    pub fn run(&mut self) -> RunnerResult {
        self.diagnostic_writer.discovery_started();
        let discovered_tests = Discoverer::new(self.project).discover();
        self.diagnostic_writer.discovery_completed(
            discovered_tests
                .values()
                .map(std::collections::HashSet::len)
                .sum(),
        );

        let test_results = Python::with_gil(|py| {
            let add_cwd_to_sys_path_result = self.add_cwd_to_sys_path(py);

            if add_cwd_to_sys_path_result.is_err() {
                return Err("Failed to add cwd to sys.path".to_string());
            }
            Ok(discovered_tests
                .iter()
                .filter_map(|(module_name, test_cases)| {
                    let imported_module = match PyModule::import(py, module_name) {
                        Ok(module) => module,
                        Err(e) => {
                            self.diagnostic_writer
                                .error(&format!("Failed to import module {module_name}: {e}"));
                            return None;
                        }
                    };
                    let mut test_results = Vec::new();
                    for test_case in test_cases {
                        let test_name = test_case.function_definition().name.to_string();
                        let module = test_case.module();

                        self.diagnostic_writer.test_started(&test_name, module);

                        let test_result = test_case.run_test(py, &imported_module);

                        self.diagnostic_writer.test_completed(&test_result);

                        test_results.push(test_result);
                    }
                    Some(test_results)
                })
                .flatten()
                .collect())
        })
        .unwrap_or_default();

        let runner_result = RunnerResult::new(test_results);
        self.diagnostic_writer.finish(&runner_result);
        runner_result
    }

    fn add_cwd_to_sys_path(&self, py: Python) -> PyResult<()> {
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
    #[must_use]
    pub const fn new(test_results: Vec<TestResult>) -> Self {
        Self { test_results }
    }

    #[must_use]
    pub fn passed(&self) -> bool {
        self.test_results.iter().all(TestResult::is_pass)
    }

    #[must_use]
    pub fn test_results(&self) -> &[TestResult] {
        &self.test_results
    }

    #[must_use]
    pub fn stats(&self) -> RunStats {
        let mut stats = RunStats::default();
        for test_result in &self.test_results {
            stats.total += 1;
            match test_result {
                TestResult::Pass(_) => stats.passed += 1,
                TestResult::Fail(_) => stats.failed += 1,
                TestResult::Error(_) => stats.error += 1,
            }
        }
        stats
    }
}

#[derive(Debug, Default)]
pub struct RunStats {
    total: usize,
    passed: usize,
    failed: usize,
    error: usize,
}

impl RunStats {
    #[must_use]
    pub const fn total(&self) -> usize {
        self.total
    }

    #[must_use]
    pub const fn passed(&self) -> usize {
        self.passed
    }

    #[must_use]
    pub const fn failed(&self) -> usize {
        self.failed
    }

    #[must_use]
    pub const fn error(&self) -> usize {
        self.error
    }
}

#[cfg(test)]
mod tests {
    use std::{
        io::{self, Write},
        sync::{Arc, Mutex},
    };

    use tempfile::TempDir;

    use super::*;
    use crate::path::{PythonTestPath, SystemPathBuf};

    #[derive(Clone, Debug)]
    struct SharedBufferWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedBufferWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);

            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn create_test_writer() -> DiagnosticWriter {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        DiagnosticWriter::new(SharedBufferWriter(buffer))
    }

    struct TestEnv {
        temp_dir: TempDir,
    }

    impl TestEnv {
        fn new() -> Self {
            Self {
                temp_dir: TempDir::new().unwrap(),
            }
        }

        fn create_test_file(&self, filename: &str, content: &str) -> SystemPathBuf {
            let path = self.temp_dir.path().join(filename);
            std::fs::write(&path, content).unwrap();
            SystemPathBuf::from(path)
        }

        fn create_python_test_path(&self, filename: &str) -> PythonTestPath {
            let path = self.temp_dir.path().join(filename);
            PythonTestPath::new(&SystemPathBuf::from(path)).unwrap()
        }
    }

    #[test]
    fn test_runner_with_passing_test() {
        let env = TestEnv::new();
        env.create_test_file(
            "test_pass.py",
            r"
def test_simple_pass():
    assert True
",
        );

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![env.create_python_test_path("test_pass.py")],
            "test".to_string(),
        );
        let mut runner = Runner::new(&project, create_test_writer());

        let result = runner.run();

        assert_eq!(result.stats().total(), 1);
        assert_eq!(result.stats().passed(), 1);
        assert_eq!(result.stats().failed(), 0);
        assert_eq!(result.stats().error(), 0);
    }

    #[test]
    fn test_runner_with_failing_test() {
        let env = TestEnv::new();
        env.create_test_file(
            "test_fail.py",
            r#"
def test_simple_fail():
    assert False, "This test should fail"
"#,
        );

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![env.create_python_test_path("test_fail.py")],
            "test".to_string(),
        );
        let mut runner = Runner::new(&project, create_test_writer());

        let result = runner.run();

        assert_eq!(result.stats().total(), 1);
        assert_eq!(result.stats().passed(), 0);
        assert_eq!(result.stats().failed(), 1);
        assert_eq!(result.stats().error(), 0);
    }

    #[test]
    fn test_runner_with_error_test() {
        let env = TestEnv::new();
        env.create_test_file(
            "test_error.py",
            r#"
def test_simple_error():
    raise ValueError("This is an error")
"#,
        );

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![env.create_python_test_path("test_error.py")],
            "test".to_string(),
        );
        let mut runner = Runner::new(&project, create_test_writer());

        let result = runner.run();

        assert_eq!(result.stats().total(), 1);
        assert_eq!(result.stats().passed(), 0);
        assert_eq!(result.stats().failed(), 0);
        assert_eq!(result.stats().error(), 1);
    }

    #[test]
    fn test_runner_with_multiple_tests() {
        let env = TestEnv::new();
        env.create_test_file(
            "test_mixed.py",
            r#"def test_pass():
    assert True

def test_fail():
    assert False, "This test should fail"

def test_error():
    raise ValueError("This is an error")
"#,
        );

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![env.create_python_test_path("test_mixed.py")],
            "test".to_string(),
        );
        let mut runner = Runner::new(&project, create_test_writer());

        let result = runner.run();

        assert_eq!(result.stats().total(), 3);
        assert_eq!(result.stats().passed(), 1);
        assert_eq!(result.stats().failed(), 1);
        assert_eq!(result.stats().error(), 1);
    }
}
