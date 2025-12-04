use std::{collections::HashMap, fmt::Debug, time::Instant};

use colored::Colorize;
use ruff_db::diagnostic::{
    Diagnostic, DiagnosticFormat, DisplayDiagnosticConfig, DisplayDiagnostics,
};

use crate::{DefaultFileResolver, Reporter};

#[derive(Debug, Clone)]
pub struct TestRunResultDisplayOptions {
    pub(crate) display_diagnostics: bool,
    pub(crate) diagnostic_format: DiagnosticFormat,
}

/// Represents the result of a test run.
///
/// This is held in the test context and updated throughout the test run.
#[derive(Debug, Clone)]
pub struct TestRunResult {
    /// Diagnostics generated during test discovery.
    discovery_diagnostics: Vec<Diagnostic>,

    /// Diagnostics generated during test collection and  execution.
    diagnostics: Vec<Diagnostic>,

    /// Stats generated during test execution.
    stats: TestResultStats,

    /// Start time of the test run.
    start_time: Instant,

    /// Current working directory.
    ///
    /// This is used to resolve file paths in diagnostics.
    cwd: std::path::PathBuf,

    /// Display options
    display_options: TestRunResultDisplayOptions,
}

impl TestRunResult {
    pub fn new(cwd: std::path::PathBuf, display_options: TestRunResultDisplayOptions) -> Self {
        Self {
            discovery_diagnostics: Vec::new(),
            diagnostics: Vec::new(),
            stats: TestResultStats::default(),
            start_time: Instant::now(),
            cwd,
            display_options,
        }
    }

    pub fn total_diagnostics(&self) -> usize {
        self.discovery_diagnostics.len() + self.diagnostics.len()
    }

    pub const fn diagnostics(&self) -> &Vec<Diagnostic> {
        &self.diagnostics
    }

    pub const fn discovery_diagnostics(&self) -> &Vec<Diagnostic> {
        &self.discovery_diagnostics
    }

    pub(crate) fn add_discovery_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.discovery_diagnostics.push(diagnostic);
    }

    pub(crate) fn add_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn is_success(&self) -> bool {
        self.stats().is_success() && self.discovery_diagnostics.is_empty()
    }

    pub const fn stats(&self) -> &TestResultStats {
        &self.stats
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

    #[must_use]
    pub(crate) fn into_sorted(mut self) -> Self {
        self.diagnostics.sort_by(Diagnostic::ruff_start_ordering);
        self
    }

    pub const fn display(&self) -> DisplayTestRunResult<'_> {
        DisplayTestRunResult { result: self }
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

    pub const fn display(&self, start_time: Instant) -> DisplayTestResultStats<'_> {
        DisplayTestResultStats::new(self, start_time)
    }
}

pub struct DisplayTestRunResult<'a> {
    result: &'a TestRunResult,
}

impl std::fmt::Display for DisplayTestRunResult<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let discovery_diagnostics = self.result.discovery_diagnostics();

        let diagnostics = self.result.diagnostics();

        let config = DisplayDiagnosticConfig::default()
            .format(self.result.display_options.diagnostic_format)
            .color(colored::control::SHOULD_COLORIZE.should_colorize());

        let is_concise = matches!(
            self.result.display_options.diagnostic_format,
            DiagnosticFormat::Concise
        );

        if (!diagnostics.is_empty() || !discovery_diagnostics.is_empty())
            && self.result.stats().total() > 0
            && self.result.display_options.display_diagnostics
        {
            writeln!(f)?;
        }

        if !discovery_diagnostics.is_empty() && self.result.display_options.display_diagnostics {
            writeln!(f, "discovery diagnostics:")?;
            writeln!(f)?;
            write!(
                f,
                "{}",
                DisplayDiagnostics::new(
                    &DefaultFileResolver::new(self.result.cwd.clone()),
                    &config,
                    discovery_diagnostics,
                )
            )?;

            if is_concise {
                writeln!(f)?;
            }
        }

        if !diagnostics.is_empty() && self.result.display_options.display_diagnostics {
            writeln!(f, "diagnostics:")?;
            writeln!(f)?;
            write!(
                f,
                "{}",
                DisplayDiagnostics::new(
                    &DefaultFileResolver::new(self.result.cwd.clone()),
                    &config,
                    diagnostics,
                )
            )?;

            if is_concise {
                writeln!(f)?;
            }
        }

        if (diagnostics.is_empty() && discovery_diagnostics.is_empty())
            && self.result.stats().total() > 0
            && self.result.display_options.display_diagnostics
        {
            writeln!(f)?;
        }

        write!(f, "{}", self.result.stats().display(self.result.start_time))
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

        let elapsed = self.start_time.elapsed();
        let time_display = if elapsed.as_secs() < 2 {
            format!("{}ms", elapsed.as_millis())
        } else {
            format!("{}s", elapsed.as_millis() / 1000)
        };

        writeln!(
            f,
            ". {} passed; {} failed; {} skipped; finished in {}",
            self.stats.passed(),
            self.stats.failed(),
            self.stats.skipped(),
            time_display
        )
    }
}
