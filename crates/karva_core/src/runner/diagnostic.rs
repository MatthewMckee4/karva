use std::{collections::HashMap, time::Instant};

use colored::Colorize;

use crate::diagnostic::Diagnostic;

#[derive(Clone, Debug)]
pub struct RunDiagnostics {
    diagnostics: Vec<Diagnostic>,
    stats: DiagnosticStats,
    start_time: Instant,
}

impl Default for RunDiagnostics {
    fn default() -> Self {
        Self {
            diagnostics: Vec::new(),
            stats: DiagnosticStats::default(),
            start_time: Instant::now(),
        }
    }
}

impl RunDiagnostics {
    #[must_use]
    pub const fn diagnostics(&self) -> &Vec<Diagnostic> {
        &self.diagnostics
    }

    pub(crate) fn add_diagnostics(&mut self, diagnostics: Vec<Diagnostic>) {
        for diagnostic in diagnostics {
            self.add_diagnostic(diagnostic);
        }
    }

    pub(crate) fn add_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub(crate) fn update(&mut self, other: &Self) {
        for diagnostic in other.diagnostics.clone() {
            self.diagnostics.push(diagnostic);
        }
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
    pub const fn stats(&self) -> &DiagnosticStats {
        &self.stats
    }

    pub(crate) const fn stats_mut(&mut self) -> &mut DiagnosticStats {
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

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
pub enum DiagnosticKind {
    Passed,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticStats {
    inner: HashMap<DiagnosticKind, usize>,
}

impl DiagnosticStats {
    pub(crate) fn update(&mut self, other: &Self) {
        for (kind, count) in &other.inner {
            self.inner
                .entry(*kind)
                .and_modify(|v| *v += count)
                .or_insert(*count);
        }
    }

    #[must_use]
    pub fn total(&self) -> usize {
        self.inner.values().sum()
    }

    pub fn is_success(&self) -> bool {
        self.failed() == 0
    }

    fn get(&self, kind: DiagnosticKind) -> usize {
        self.inner.get(&kind).copied().unwrap_or(0)
    }

    #[must_use]
    pub(crate) fn passed(&self) -> usize {
        self.get(DiagnosticKind::Passed)
    }

    #[must_use]
    pub(crate) fn failed(&self) -> usize {
        self.get(DiagnosticKind::Failed)
    }

    #[must_use]
    pub(crate) fn skipped(&self) -> usize {
        self.get(DiagnosticKind::Skipped)
    }

    fn add(&mut self, kind: DiagnosticKind) {
        self.inner.entry(kind).and_modify(|v| *v += 1).or_insert(1);
    }

    pub(crate) fn add_failed(&mut self) {
        self.add(DiagnosticKind::Failed);
    }

    pub(crate) fn add_passed(&mut self) {
        self.add(DiagnosticKind::Passed);
    }

    pub(crate) fn add_skipped(&mut self) {
        self.add(DiagnosticKind::Skipped);
    }
}

pub struct DisplayRunDiagnostics<'a> {
    diagnostics: &'a RunDiagnostics,
}

impl<'a> DisplayRunDiagnostics<'a> {
    pub(crate) const fn new(diagnostics: &'a RunDiagnostics) -> Self {
        Self { diagnostics }
    }
}

impl std::fmt::Display for DisplayRunDiagnostics<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stats = self.diagnostics.stats();

        let success = stats.is_success();

        write!(f, "test result: ")?;

        if success {
            write!(f, "{}", "ok".green())?;
        } else {
            write!(f, "{}", "FAILED".red())?;
        }

        writeln!(
            f,
            ". {} passed; {} failed; {} skipped; finished in {}s",
            stats.passed(),
            stats.failed(),
            stats.skipped(),
            self.diagnostics.start_time.elapsed().as_millis() / 1000
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
        let mut diagnostics = RunDiagnostics::default();
        diagnostics.stats_mut().add_passed();
        diagnostics.stats_mut().add_passed();
        diagnostics.stats_mut().add_passed();

        let output = diagnostics.display().to_string();
        assert_snapshot_filtered!(output, @"test result: ok. 3 passed; 0 failed; 0 skipped; finished in [TIME]");
    }

    #[test]
    fn test_display_with_failures() {
        let mut diagnostics = RunDiagnostics::default();
        diagnostics.stats_mut().add_passed();
        diagnostics.stats_mut().add_failed();
        diagnostics.stats_mut().add_failed();

        let output = diagnostics.display().to_string();
        assert_snapshot_filtered!(output, @"test result: FAILED. 1 passed; 2 failed; 0 skipped; finished in [TIME]");
    }

    #[test]
    fn test_display_with_skipped() {
        let mut diagnostics = RunDiagnostics::default();
        diagnostics.stats_mut().add_passed();
        diagnostics.stats_mut().add_skipped();
        diagnostics.stats_mut().add_skipped();

        let output = diagnostics.display().to_string();
        assert_snapshot_filtered!(output, @"test result: ok. 1 passed; 0 failed; 2 skipped; finished in [TIME]");
    }

    #[test]
    fn test_display_mixed_results() {
        let mut diagnostics = RunDiagnostics::default();
        diagnostics.stats_mut().add_passed();
        diagnostics.stats_mut().add_passed();
        diagnostics.stats_mut().add_failed();
        diagnostics.stats_mut().add_skipped();

        let output = diagnostics.display().to_string();
        assert_snapshot_filtered!(output, @"test result: FAILED. 2 passed; 1 failed; 1 skipped; finished in [TIME]");
    }

    #[test]
    fn test_display_no_tests() {
        let diagnostics = RunDiagnostics::default();

        let output = diagnostics.display().to_string();
        assert_snapshot_filtered!(output, @"test result: ok. 0 passed; 0 failed; 0 skipped; finished in [TIME]");
    }

    #[test]
    fn test_diagnostic_stats_totals() {
        let mut stats = DiagnosticStats::default();
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

    #[test]
    fn test_run_diagnostics_update() {
        let mut diagnostics1 = RunDiagnostics::default();
        diagnostics1.stats_mut().add_passed();

        let mut diagnostics2 = RunDiagnostics::default();
        diagnostics2.stats_mut().add_failed();

        diagnostics1.update(&diagnostics2);

        assert_eq!(diagnostics1.stats().passed(), 1);
        assert_eq!(diagnostics1.stats().failed(), 1);
    }
}
