use std::io::Write;

use colored::{Color, Colorize};

use crate::{
    case::{TestFunctionRunResult, TestFunctionRunStats},
    diagnostic::Diagnostic,
};

#[derive(Clone, Debug, Default)]
pub struct RunDiagnostics {
    pub diagnostics: Vec<Diagnostic>,
    pub stats: DiagnosticStats,
}

impl RunDiagnostics {
    pub fn add_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        self.diagnostics.extend(diagnostics);
    }

    pub fn add_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub const fn add_stats(&mut self, stats: &TestFunctionRunStats) {
        self.stats.add_test_function_stats(stats);
    }

    pub fn report_test_function_run_result(
        &mut self,
        test_function_run_result: TestFunctionRunResult,
    ) {
        self.add_diagnostics(test_function_run_result.diagnostics);
        self.add_stats(&test_function_run_result.result);
    }

    pub fn update(&mut self, other: &Self) {
        self.diagnostics.extend(other.diagnostics.clone());
        self.stats.update(&other.stats);
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
    pub const fn stats(&self) -> &DiagnosticStats {
        &self.stats
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
            for (label, num, color) in [
                ("Passed tests:", stats.passed(), Color::Green),
                ("Failed tests:", stats.failed(), Color::Red),
                ("Errored tests:", stats.errored(), Color::Yellow),
            ] {
                Self::log_test_count(writer, label, num, color);
            }
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter()
    }
}

#[derive(Debug, Clone, Default)]
pub struct DiagnosticStats {
    total: usize,
    passed: usize,
    failed: usize,
    errored: usize,
}

impl DiagnosticStats {
    pub const fn add_test_function_stats(&mut self, stats: &TestFunctionRunStats) {
        self.total += stats.total();
        self.passed += stats.passed();
        self.failed += stats.failed();
        self.errored += stats.errored();
    }

    pub const fn update(&mut self, other: &Self) {
        self.total += other.total();
        self.passed += other.passed();
        self.failed += other.failed();
        self.errored += other.errored();
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
    pub const fn errored(&self) -> usize {
        self.errored
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
        assert_eq!(result.stats().errored(), 0);
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
        assert_eq!(result.stats().errored(), 0);
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
        assert_eq!(result.stats().errored(), 1);
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
        assert_eq!(result.stats().errored(), 1);
    }
}
