use std::io::Write;

use colored::{Color, Colorize};
use karva_project::project::Project;
use pyo3::prelude::*;

use crate::{
    diagnostic::reporter::{DummyReporter, Reporter},
    discovery::Discoverer,
    test_result::TestResult,
};

pub struct TestRunner<'a> {
    project: &'a Project,
}

impl<'a> TestRunner<'a> {
    #[must_use]
    pub const fn new(project: &'a Project) -> Self {
        Self { project }
    }

    pub fn run(&self) -> RunDiagnostics {
        self.run_impl(&mut DummyReporter)
    }

    pub fn run_with_reporter(&self, reporter: &mut dyn Reporter) -> RunDiagnostics {
        self.run_impl(reporter)
    }

    #[must_use]
    fn run_impl(&self, reporter: &mut dyn Reporter) -> RunDiagnostics {
        let discovered_tests = Discoverer::new(self.project).discover();

        reporter.set_files(discovered_tests.count);

        let test_results = Python::with_gil(|py| {
            let add_cwd_to_sys_path_result = self.add_cwd_to_sys_path(py);

            if add_cwd_to_sys_path_result.is_err() {
                return Err("Failed to add cwd to sys.path".to_string());
            }

            Ok(discovered_tests
                .tests
                .iter()
                .filter_map(|(module_name, test_cases)| {
                    let res = {
                        let imported_module = match PyModule::import(py, module_name) {
                            Ok(module) => module,
                            Err(e) => {
                                tracing::error!("Failed to import module {module_name}: {e}");
                                return None;
                            }
                        };

                        Some(
                            test_cases
                                .iter()
                                .map(|test_case| test_case.run_test(py, &imported_module))
                                .collect::<Vec<_>>(),
                        )
                    };
                    reporter.report_file(module_name);
                    res
                })
                .flatten()
                .collect())
        })
        .unwrap_or_default();

        RunDiagnostics::new(test_results)
    }

    fn add_cwd_to_sys_path(&self, py: Python) -> PyResult<()> {
        let sys_path = py.import("sys")?;
        let path = sys_path.getattr("path")?;
        path.call_method1("append", (self.project.cwd().as_str(),))?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct RunDiagnostics {
    test_results: Vec<TestResult>,
}

impl RunDiagnostics {
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

    fn log_test_count(writer: &mut dyn Write, label: &str, count: usize, color: Color) {
        if count > 0 {
            let _ = writeln!(
                writer,
                "{} {}",
                label.color(color),
                count.to_string().color(color)
            );
        }
    }

    pub fn display(&self, writer: &mut dyn Write) {
        let stats = self.stats();

        if stats.total() > 0 {
            let _ = writeln!(writer);
            // self.display_test_results(&mut writer);
            let _ = writeln!(writer, "{}", "─────────────".bold());
            for (label, num, color) in [
                ("Passed tests:", stats.passed(), Color::Green),
                ("Failed tests:", stats.failed(), Color::Red),
                ("Error tests:", stats.error(), Color::Yellow),
            ] {
                Self::log_test_count(writer, label, num, color);
            }
        }
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

    use karva_project::path::SystemPathBuf;
    use tempfile::TempDir;

    use super::*;

    struct TestEnv {
        temp_dir: TempDir,
    }

    impl TestEnv {
        fn new() -> Self {
            Self {
                temp_dir: TempDir::new().unwrap(),
            }
        }

        fn create_test_file(&self, filename: &str, content: &str) -> String {
            let path = self.temp_dir.path().join(filename);
            std::fs::write(&path, content).unwrap();
            path.display().to_string()
        }

        fn create_python_test_path(&self, filename: &str) -> String {
            let path = self.temp_dir.path().join(filename);
            path.display().to_string()
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
        );
        let runner = TestRunner::new(&project);

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
        );
        let runner = TestRunner::new(&project);

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
        );
        let runner = TestRunner::new(&project);

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
        );
        let runner = TestRunner::new(&project);

        let result = runner.run();

        assert_eq!(result.stats().total(), 3);
        assert_eq!(result.stats().passed(), 1);
        assert_eq!(result.stats().failed(), 1);
        assert_eq!(result.stats().error(), 1);
    }
}
