use pyo3::prelude::*;

use crate::{
    diagnostics::DiagnosticWriter, discovery::Discoverer, project::Project, test_result::TestResult,
};

pub struct Runner<'a> {
    project: &'a Project,
    diagnostic_writer: DiagnosticWriter,
}

impl<'a> Runner<'a> {
    pub fn new(project: &'a Project, diagnostics: DiagnosticWriter) -> Self {
        Self {
            project,
            diagnostic_writer: diagnostics,
        }
    }

    pub fn diagnostic_writer(&self) -> &DiagnosticWriter {
        &self.diagnostic_writer
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
                    let test_name = test.function_definition().name.to_string();
                    let module = test.module();

                    self.diagnostic_writer.test_started(&test_name, module);

                    let test_result = test.run_test(&py);

                    self.diagnostic_writer.test_completed(&test_result);

                    test_result
                })
                .collect())
        })
        .unwrap_or_default();

        let runner_result = RunnerResult::new(test_results);
        self.diagnostic_writer.finish(&runner_result);
        runner_result
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
            let mut inner = self.0.lock().unwrap();
            inner.extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn create_test_writer() -> DiagnosticWriter {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        DiagnosticWriter::new(SharedBufferWriter(buffer.clone()))
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
            r#"
def test_simple_pass():
    assert True
"#,
        );

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![env.create_python_test_path("test_pass.py")],
            "test".to_string(),
        );
        let mut runner = Runner::new(&project, create_test_writer());

        let result = runner.run();

        assert_eq!(result.stats().total_tests(), 1);
        assert_eq!(result.stats().passed_tests(), 1);
        assert_eq!(result.stats().failed_tests(), 0);
        assert_eq!(result.stats().error_tests(), 0);
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

        assert_eq!(result.stats().total_tests(), 1);
        assert_eq!(result.stats().passed_tests(), 0);
        assert_eq!(result.stats().failed_tests(), 1);
        assert_eq!(result.stats().error_tests(), 0);
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

        assert_eq!(result.stats().total_tests(), 1);
        assert_eq!(result.stats().passed_tests(), 0);
        assert_eq!(result.stats().failed_tests(), 0);
        assert_eq!(result.stats().error_tests(), 1);
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

        assert_eq!(result.stats().total_tests(), 3);
        assert_eq!(result.stats().passed_tests(), 1);
        assert_eq!(result.stats().failed_tests(), 1);
        assert_eq!(result.stats().error_tests(), 1);
    }
}
