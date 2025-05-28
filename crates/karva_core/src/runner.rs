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
                    let test_name = test.function_name().as_str();
                    let module = test.module();

                    self.diagnostic_writer.test_started(test_name, module);

                    let test_result = self.run_test(&py, test);

                    match test_result {
                        Ok(test_result) => {
                            self.diagnostic_writer.test_completed(&test_result);
                            test_result
                        }
                        Err(error) => {
                            let error = TestResult::Error(error);
                            self.diagnostic_writer.test_completed(&error);
                            error
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

    fn run_test(&self, py: &Python, test: &DiscoveredTest) -> Result<TestResult, TestResultError> {
        let start_time = Instant::now();

        let imported_module =
            PyModule::import(*py, test.module()).map_err(|e| TestResultError {
                test: test.clone(),
                traceback: e.to_string(),
                duration: start_time.elapsed(),
            })?;
        let function =
            imported_module
                .getattr(test.function_name())
                .map_err(|e| TestResultError {
                    test: test.clone(),
                    traceback: e.to_string(),
                    duration: start_time.elapsed(),
                })?;

        let result = function.call0();
        let duration = start_time.elapsed();

        match result {
            Ok(_) => Ok(TestResult::new_pass(test.clone(), duration)),
            Err(err) => {
                let err_value = err.value(*py);
                if err_value.is_instance_of::<PyAssertionError>() {
                    let traceback = err
                        .traceback(*py)
                        .map(|traceback| filter_traceback(&traceback.format().unwrap_or_default()));
                    Ok(TestResult::new_fail(test.clone(), traceback, duration))
                } else {
                    let traceback = err
                        .traceback(*py)
                        .map(|traceback| filter_traceback(&traceback.format().unwrap_or_default()))
                        .unwrap_or_default();
                    Ok(TestResult::new_error(test.clone(), traceback, duration))
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

fn filter_traceback(traceback: &str) -> String {
    let lines: Vec<&str> = traceback.lines().collect();
    let mut filtered = String::new();

    for (i, line) in lines.iter().enumerate() {
        if i == 0 && line.contains("Traceback (most recent call last):") {
            continue;
        }
        if line.starts_with("  ") {
            if let Some(stripped) = line.strip_prefix("  ") {
                filtered.push_str(stripped);
            }
        } else {
            filtered.push_str(line);
        }
        filtered.push('\n');
    }

    filtered.trim_end().to_string()
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
    use crate::path::{PythonTestPath, SystemPathBuf};
    use regex::Regex;
    use std::{
        io::{self, Write},
        sync::{Arc, Mutex},
    };
    use tempfile::TempDir;

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

        fn temp_dir_path(&self) -> &std::path::Path {
            self.temp_dir.path()
        }
    }

    fn create_test_writer() -> (DiagnosticWriter, Arc<Mutex<Vec<u8>>>) {
        let buffer = Arc::new(Mutex::new(Vec::new()));
        let writer = DiagnosticWriter::new(SharedBufferWriter(buffer.clone()));
        (writer, buffer)
    }

    fn strip_ansi_codes(s: &str) -> String {
        let re = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
        re.replace_all(s, "").to_string()
    }

    // Replace temporary directory paths with a placeholder
    fn normalize_output(output: &str, temp_dir: &std::path::Path) -> String {
        let temp_dir_str = temp_dir.to_string_lossy();
        let re = Regex::new(&format!("{}/", regex::escape(&temp_dir_str))).unwrap();
        re.replace_all(output, "/PLACEHOLDER/").to_string()
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
        let (diagnostic_writer, buffer) = create_test_writer();
        let mut runner = Runner::new(&project, diagnostic_writer);

        runner.run();

        let output = buffer.lock().unwrap();
        let stdout = strip_ansi_codes(&String::from_utf8(output.clone()).unwrap());
        let normalized_output = normalize_output(&stdout, env.temp_dir_path());
        let expected_output = r#"Discovered 1 tests
.
─────────────
Passed tests: 1
"#;
        assert_eq!(normalized_output, expected_output);
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
        let (diagnostic_writer, buffer) = create_test_writer();
        let mut runner = Runner::new(&project, diagnostic_writer);

        runner.run();

        let output = buffer.lock().unwrap();
        let stdout = strip_ansi_codes(&String::from_utf8(output.clone()).unwrap());
        let normalized_output = normalize_output(&stdout, env.temp_dir_path());
        let expected_output = r#"Discovered 1 tests
.
Failed tests:
test_fail::test_simple_fail
File "/PLACEHOLDER/test_fail.py", line 3, in test_simple_fail
  assert False, "This test should fail"
─────────────
Failed tests: 1
"#;
        assert_eq!(normalized_output, expected_output);
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
        let (diagnostic_writer, buffer) = create_test_writer();
        let mut runner = Runner::new(&project, diagnostic_writer);

        runner.run();

        let output = buffer.lock().unwrap();
        let stdout = strip_ansi_codes(&String::from_utf8(output.clone()).unwrap());
        let normalized_output = normalize_output(&stdout, env.temp_dir_path());
        let expected_output = r#"Discovered 1 tests
.
Error tests:
test_error::test_simple_error
File "/PLACEHOLDER/test_error.py", line 3, in test_simple_error
  raise ValueError("This is an error")
─────────────
Error tests: 1
"#;
        assert_eq!(normalized_output, expected_output);
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
        let (diagnostic_writer, buffer) = create_test_writer();
        let mut runner = Runner::new(&project, diagnostic_writer);

        runner.run();

        let output = buffer.lock().unwrap();
        let stdout = strip_ansi_codes(&String::from_utf8(output.clone()).unwrap());
        let normalized_output = normalize_output(&stdout, env.temp_dir_path());
        let expected_output = r#"Discovered 3 tests
...
Failed tests:
test_mixed::test_fail
File "/PLACEHOLDER/test_mixed.py", line 5, in test_fail
  assert False, "This test should fail"
Error tests:
test_mixed::test_error
File "/PLACEHOLDER/test_mixed.py", line 8, in test_error
  raise ValueError("This is an error")
─────────────
Passed tests: 1
Failed tests: 1
Error tests: 1
"#;
        assert_eq!(normalized_output, expected_output);
    }

    #[test]
    fn test_runner_with_invalid_module() {
        let env = TestEnv::new();
        env.create_test_file(
            "invalid.py",
            r#"
def test_invalid():
    this_is_not_defined
"#,
        );

        let project = Project::new(
            SystemPathBuf::from(env.temp_dir.path()),
            vec![env.create_python_test_path("invalid.py")],
            "test".to_string(),
        );
        let (diagnostic_writer, buffer) = create_test_writer();
        let mut runner = Runner::new(&project, diagnostic_writer);

        runner.run();

        let output = buffer.lock().unwrap();
        let stdout = strip_ansi_codes(&String::from_utf8(output.clone()).unwrap());
        let normalized_output = normalize_output(&stdout, env.temp_dir_path());
        let expected_output = r#"Discovered 1 tests
.
Error tests:
invalid::test_invalid
File "/PLACEHOLDER/invalid.py", line 3, in test_invalid
  this_is_not_defined
─────────────
Error tests: 1
"#;
        assert_eq!(normalized_output, expected_output);
    }
}
