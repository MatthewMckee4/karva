use std::io::Write;

use colored::{Color, Colorize};

use crate::diagnostic::{Diagnostic, DiagnosticScope, SubDiagnosticType};

#[derive(Clone)]
pub struct RunDiagnostics {
    pub diagnostics: Vec<Diagnostic>,
    pub total_tests: usize,
}

impl RunDiagnostics {
    #[must_use]
    pub const fn new(test_results: Vec<Diagnostic>, total_tests: usize) -> Self {
        Self {
            diagnostics: test_results,
            total_tests,
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    #[must_use]
    pub fn test_results(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.diagnostics.len()
    }

    #[must_use]
    pub fn stats(&self) -> DiagnosticStats {
        let mut stats = DiagnosticStats::new(self.total_tests);
        for diagnostic in &self.diagnostics {
            if diagnostic.scope() == &DiagnosticScope::Test {
                stats.passed -= 1;
                match diagnostic.diagnostic_type() {
                    SubDiagnosticType::Fail => stats.failed += 1,
                    SubDiagnosticType::Error(_) => stats.error += 1,
                }
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
            let _ = writeln!(writer, "{}", "-------------".bold());
            for (label, num, color) in [
                ("Passed tests:", stats.passed(), Color::Green),
                ("Failed tests:", stats.failed(), Color::Red),
                ("Error tests:", stats.error(), Color::Yellow),
            ] {
                Self::log_test_count(writer, label, num, color);
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter()
    }
}

#[derive(Debug)]
pub struct DiagnosticStats {
    total: usize,
    passed: usize,
    failed: usize,
    error: usize,
}

impl DiagnosticStats {
    const fn new(total: usize) -> Self {
        Self {
            total,
            passed: total,
            failed: 0,
            error: 0,
        }
    }
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
    use karva_project::{project::Project, tests::TestEnv};

    use crate::runner::{StandardTestRunner, TestRunner};

    #[test]
    fn test_runner_with_passing_test() {
        let env = TestEnv::new();
        env.create_file(
            "test_pass.py",
            r"
def test_simple_pass():
    assert True
",
        );

        let project = Project::new(env.cwd(), vec![env.temp_path("test_pass.py")]);
        let runner = StandardTestRunner::new(&project);

        let result = runner.test();

        assert_eq!(result.stats().total(), 1);
        assert_eq!(result.stats().passed(), 1);
        assert_eq!(result.stats().failed(), 0);
        assert_eq!(result.stats().error(), 0);
    }

    #[test]
    fn test_runner_with_failing_test() {
        let env = TestEnv::new();
        env.create_file(
            "test_fail.py",
            r#"
def test_simple_fail():
    assert False, "This test should fail"
"#,
        );

        let project = Project::new(env.cwd(), vec![env.temp_path("test_fail.py")]);
        let runner = StandardTestRunner::new(&project);

        let result = runner.test();

        assert_eq!(result.stats().total(), 1);
        assert_eq!(result.stats().passed(), 0);
        assert_eq!(result.stats().failed(), 1);
        assert_eq!(result.stats().error(), 0);
    }

    #[test]
    fn test_runner_with_error_test() {
        let env = TestEnv::new();
        env.create_file(
            "test_error.py",
            r#"
def test_simple_error():
    raise ValueError("This is an error")
"#,
        );

        let project = Project::new(env.cwd(), vec![env.temp_path("test_error.py")]);
        let runner = StandardTestRunner::new(&project);

        let result = runner.test();

        assert_eq!(result.stats().total(), 1);
        assert_eq!(result.stats().passed(), 0);
        assert_eq!(result.stats().failed(), 0);
        assert_eq!(result.stats().error(), 1);
    }

    #[test]
    fn test_runner_with_multiple_tests() {
        let env = TestEnv::new();
        env.create_file(
            "test_mixed.py",
            r#"def test_pass():
    assert True

def test_fail():
    assert False, "This test should fail"

def test_error():
    raise ValueError("This is an error")
"#,
        );

        let project = Project::new(env.cwd(), vec![env.temp_path("test_mixed.py")]);
        let runner = StandardTestRunner::new(&project);

        let result = runner.test();

        assert_eq!(result.stats().total(), 3);
        assert_eq!(result.stats().passed(), 1);
        assert_eq!(result.stats().failed(), 1);
        assert_eq!(result.stats().error(), 1);
    }
}
