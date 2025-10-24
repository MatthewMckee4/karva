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
