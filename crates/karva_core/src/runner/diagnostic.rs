use colored::{Color, Colorize};

use crate::diagnostic::{Diagnostic, DiagnosticScope};

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
        if *diagnostic.scope() == DiagnosticScope::Test {
            let error_type = diagnostic.severity();
            if error_type.is_test_fail() {
                self.stats.add_failed();
            } else if error_type.is_test_error() {
                self.stats.add_errored();
            }
        }
        self.diagnostics.push(diagnostic);
    }

    pub const fn add_stats(&mut self, stats: &DiagnosticStats) {
        self.stats.update(stats);
    }

    pub fn update(&mut self, other: &Self) {
        self.diagnostics.extend(other.diagnostics.clone());
        self.stats.update(&other.stats);
    }

    #[must_use]
    pub fn passed(&self) -> bool {
        for diagnostic in &self.diagnostics {
            if diagnostic.severity().is_error() {
                return false;
            }
        }
        true
    }

    #[must_use]
    pub fn test_results(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    #[must_use]
    pub const fn stats(&self) -> &DiagnosticStats {
        &self.stats
    }

    pub const fn stats_mut(&mut self) -> &mut DiagnosticStats {
        &mut self.stats
    }

    pub fn iter(&self) -> impl Iterator<Item = &Diagnostic> {
        self.diagnostics.iter()
    }

    #[must_use]
    pub const fn display(&self) -> DisplayRunDiagnostics<'_> {
        DisplayRunDiagnostics::new(self)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticStats {
    total: usize,
    passed: usize,
    failed: usize,
    errored: usize,
}

impl DiagnosticStats {
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

    pub const fn add_failed(&mut self) {
        self.failed += 1;
        self.total += 1;
    }

    pub const fn add_errored(&mut self) {
        self.errored += 1;
        self.total += 1;
    }

    pub const fn add_passed(&mut self) {
        self.passed += 1;
        self.total += 1;
    }
}

pub struct DisplayRunDiagnostics<'a> {
    diagnostics: &'a RunDiagnostics,
}

impl<'a> DisplayRunDiagnostics<'a> {
    pub const fn new(diagnostics: &'a RunDiagnostics) -> Self {
        Self { diagnostics }
    }

    fn log_test_count(f: &mut std::fmt::Formatter<'_>, label: &str, count: usize, color: Color) {
        if count > 0 {
            let _ = writeln!(
                f,
                "{} {}",
                label.color(color),
                count.to_string().color(color)
            );
        }
    }
}

impl std::fmt::Display for DisplayRunDiagnostics<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stats = self.diagnostics.stats();

        if stats.total() > 0 {
            for (label, num, color) in [
                ("Passed tests:", stats.passed(), Color::Green),
                ("Failed tests:", stats.failed(), Color::Red),
                ("Errored tests:", stats.errored(), Color::Yellow),
            ] {
                Self::log_test_count(f, label, num, color);
            }
        }

        Ok(())
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
