use std::{collections::HashMap, fmt::Debug, time::Instant};

use colored::Colorize;

use crate::{
    Reporter,
    diagnostic::{Diagnostic, DiscoveryDiagnostic, DisplayOptions, FunctionDefinitionLocation},
};

#[derive(Debug, Clone)]
pub struct TestRunResult {
    discovery_diagnostics: Vec<DiscoveryDiagnostic>,
    test_diagnostics: Vec<Diagnostic>,

    stats: TestResultStats,
    start_time: Instant,
}

impl Default for TestRunResult {
    fn default() -> Self {
        Self {
            discovery_diagnostics: Vec::new(),
            test_diagnostics: Vec::new(),
            stats: TestResultStats::default(),
            start_time: Instant::now(),
        }
    }
}

impl TestRunResult {
    pub fn total_diagnostics(&self) -> usize {
        self.discovery_diagnostics.len() + self.test_diagnostics.len()
    }

    pub const fn diagnostics(&self) -> &Vec<Diagnostic> {
        &self.test_diagnostics
    }

    pub(crate) fn add_test_diagnostics(
        &mut self,
        diagnostics: impl IntoIterator<Item = Diagnostic>,
    ) {
        for diagnostic in diagnostics {
            self.test_diagnostics.push(diagnostic);
        }
    }

    pub(crate) fn add_test_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.test_diagnostics.push(diagnostic);
    }

    pub(crate) const fn discovery_diagnostics(&self) -> &Vec<DiscoveryDiagnostic> {
        &self.discovery_diagnostics
    }

    pub(crate) fn add_discovery_diagnostics(&mut self, diagnostics: Vec<DiscoveryDiagnostic>) {
        for diagnostic in diagnostics {
            self.discovery_diagnostics.push(diagnostic);
        }
    }

    pub fn passed(&self) -> bool {
        for diagnostic in &self.test_diagnostics {
            if diagnostic.is_test_failure() {
                return false;
            }
        }
        true
    }

    pub const fn stats(&self) -> &TestResultStats {
        &self.stats
    }

    #[cfg(test)]
    pub(crate) const fn stats_mut(&mut self) -> &mut TestResultStats {
        &mut self.stats
    }

    pub fn register_test_case_result(
        &mut self,
        test_case_name: &str,
        result: IndividualTestResultKind,
        reporter: Option<&dyn Reporter>,
    ) {
        self.stats.add(result.clone().into());
        if let Some(reporter) = reporter {
            reporter.report_test_case_result(test_case_name, result);
        }
    }

    pub fn display(&self) -> DisplayTestRunResult<'_> {
        self.display_with(DisplayOptions::default())
    }

    pub const fn display_with(&self, options: DisplayOptions) -> DisplayTestRunResult<'_> {
        DisplayTestRunResult::new(self, options)
    }
}

#[derive(Debug, Clone)]
pub enum IndividualTestResultKind {
    Passed,
    Failed,
    Skipped { reason: Option<String> },
}

impl From<IndividualTestResultKind> for TestResultKind {
    fn from(val: IndividualTestResultKind) -> Self {
        match val {
            IndividualTestResultKind::Passed => Self::Passed,
            IndividualTestResultKind::Failed => Self::Failed,
            IndividualTestResultKind::Skipped { .. } => Self::Skipped,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
pub enum TestResultKind {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestResultStats {
    inner: HashMap<TestResultKind, usize>,
}

impl TestResultStats {
    pub fn total(&self) -> usize {
        self.inner.values().sum()
    }

    pub fn is_success(&self) -> bool {
        self.failed() == 0
    }

    fn get(&self, kind: TestResultKind) -> usize {
        self.inner.get(&kind).copied().unwrap_or(0)
    }

    pub(crate) fn passed(&self) -> usize {
        self.get(TestResultKind::Passed)
    }

    pub(crate) fn failed(&self) -> usize {
        self.get(TestResultKind::Failed)
    }

    pub(crate) fn skipped(&self) -> usize {
        self.get(TestResultKind::Skipped)
    }

    pub(crate) fn add(&mut self, kind: TestResultKind) {
        self.inner.entry(kind).and_modify(|v| *v += 1).or_insert(1);
    }

    pub fn add_failed(&mut self) {
        self.add(TestResultKind::Failed);
    }

    pub fn add_passed(&mut self) {
        self.add(TestResultKind::Passed);
    }

    pub fn add_skipped(&mut self) {
        self.add(TestResultKind::Skipped);
    }

    pub const fn display(&self, start_time: Instant) -> DisplayTestResultStats<'_> {
        DisplayTestResultStats::new(self, start_time)
    }
}

pub struct DisplayTestRunResult<'a> {
    test_run_result: &'a TestRunResult,
    options: DisplayOptions,
}

impl<'a> DisplayTestRunResult<'a> {
    pub(crate) const fn new(test_run_result: &'a TestRunResult, options: DisplayOptions) -> Self {
        Self {
            test_run_result,
            options,
        }
    }
}

impl std::fmt::Display for DisplayTestRunResult<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.test_run_result.discovery_diagnostics().is_empty() {
            writeln!(f, "discovery failures:").ok();

            writeln!(f).ok();

            for diagnostic in self.test_run_result.discovery_diagnostics() {
                writeln!(f, "{}", diagnostic.display()).ok();
            }
        }

        let test_failures = self
            .test_run_result
            .diagnostics()
            .iter()
            .filter(|d| d.is_test_failure())
            .collect::<Vec<_>>();

        let fixture_failures = self
            .test_run_result
            .diagnostics()
            .iter()
            .filter(|d| d.is_fixture_failure())
            .collect::<Vec<_>>();

        let warnings = self
            .test_run_result
            .diagnostics()
            .iter()
            .filter(|d| d.is_warning())
            .collect::<Vec<_>>();

        if !fixture_failures.is_empty() {
            writeln!(f, "fixture failures:")?;
            writeln!(f)?;

            for diagnostic in &fixture_failures {
                writeln!(f, "{}", diagnostic.display_with(self.options))?;
            }
        }

        if !test_failures.is_empty() {
            writeln!(f, "test failures:")?;
            writeln!(f)?;

            for diagnostic in &test_failures {
                writeln!(f, "{}", diagnostic.display_with(self.options))?;
            }
        }

        if !warnings.is_empty() {
            writeln!(f, "warnings:")?;
            writeln!(f)?;

            for diagnostic in &warnings {
                writeln!(f, "{}", diagnostic.display_with(self.options))?;
            }
        }

        if !test_failures.is_empty() {
            writeln!(f, "test failures:")?;

            for diagnostic in &test_failures {
                let Some(FunctionDefinitionLocation {
                    location,
                    function_name,
                }) = diagnostic.location()
                else {
                    continue;
                };

                let location_string = location
                    .as_ref()
                    .map(|location| format!(" at {location}"))
                    .unwrap_or_default();

                writeln!(f, "    {function_name}{location_string}")?;
            }

            writeln!(f)?;
        }

        write!(
            f,
            "{}",
            self.test_run_result
                .stats()
                .display(self.test_run_result.start_time)
        )
    }
}

pub struct DisplayTestResultStats<'a> {
    stats: &'a TestResultStats,
    start_time: Instant,
}

impl<'a> DisplayTestResultStats<'a> {
    const fn new(stats: &'a TestResultStats, start_time: Instant) -> Self {
        Self { stats, start_time }
    }
}

impl std::fmt::Display for DisplayTestResultStats<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let success = self.stats.is_success();

        write!(f, "test result: ")?;

        if success {
            write!(f, "{}", "ok".green())?;
        } else {
            write!(f, "{}", "FAILED".red())?;
        }

        writeln!(
            f,
            ". {} passed; {} failed; {} skipped; finished in {}s",
            self.stats.passed(),
            self.stats.failed(),
            self.stats.skipped(),
            self.start_time.elapsed().as_millis() / 1000
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! assert_snapshot_filtered {
        ($value:expr, @$snapshot:literal) => {
            insta::with_settings!({
                filters => vec![
                    (r"\x1b\[\d+m", ""),
                    (r"(\s|\()(\d+m )?(\d+\.)?\d+(ms|s)", "$1[TIME]"),
                ]
            }, {
                insta::assert_snapshot!($value, @$snapshot);
            });
        };
    }

    #[test]
    fn test_display_all_passed() {
        let mut diagnostics = TestRunResult::default();
        diagnostics.stats_mut().add_passed();
        diagnostics.stats_mut().add_passed();
        diagnostics.stats_mut().add_passed();

        let output = diagnostics.display().to_string();
        assert_snapshot_filtered!(output, @"test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]");
    }

    #[test]
    fn test_display_with_failures() {
        let mut diagnostics = TestRunResult::default();
        diagnostics.stats_mut().add_passed();
        diagnostics.stats_mut().add_failed();
        diagnostics.stats_mut().add_failed();

        let output = diagnostics.display().to_string();
        assert_snapshot_filtered!(output, @"test result: FAILED. 1 passed; 2 failed; 0 skipped; finished in [TIME]");
    }

    #[test]
    fn test_display_with_skipped() {
        let mut diagnostics = TestRunResult::default();
        diagnostics.stats_mut().add_passed();
        diagnostics.stats_mut().add_skipped();
        diagnostics.stats_mut().add_skipped();

        let output = diagnostics.display().to_string();
        assert_snapshot_filtered!(output, @"test result: ok. 1 passed; 0 failed; 2 skipped; finished in [TIME]");
    }

    #[test]
    fn test_display_mixed_results() {
        let mut diagnostics = TestRunResult::default();
        diagnostics.stats_mut().add_passed();
        diagnostics.stats_mut().add_passed();
        diagnostics.stats_mut().add_failed();
        diagnostics.stats_mut().add_skipped();

        let output = diagnostics.display().to_string();
        assert_snapshot_filtered!(output, @"test result: FAILED. 2 passed; 1 failed; 1 skipped; finished in [TIME]");
    }

    #[test]
    fn test_display_no_tests() {
        let diagnostics = TestRunResult::default();

        let output = diagnostics.display().to_string();
        assert_snapshot_filtered!(output, @"test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]");
    }

    #[test]
    fn test_diagnostic_stats_totals() {
        let mut stats = TestResultStats::default();
        stats.add_passed();
        stats.add_passed();
        stats.add_failed();
        stats.add_skipped();

        assert_eq!(stats.total(), 4);
        assert_eq!(stats.passed(), 2);
        assert_eq!(stats.failed(), 1);
        assert_eq!(stats.skipped(), 1);
        assert!(!stats.is_success());
    }
}
