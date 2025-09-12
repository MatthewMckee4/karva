use colored::Colorize;

use crate::diagnostic::Diagnostic;

#[derive(Clone, Debug, Default)]
pub struct RunDiagnostics {
    diagnostics: Vec<Diagnostic>,
    stats: DiagnosticStats,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DiagnosticStats {
    total: usize,
    passed: usize,
    failed: usize,
}

impl DiagnosticStats {
    pub(crate) const fn update(&mut self, other: &Self) {
        self.total += other.total();
        self.passed += other.passed();
        self.failed += other.failed();
    }

    #[must_use]
    pub const fn total(&self) -> usize {
        self.total
    }

    pub const fn is_success(&self) -> bool {
        self.failed == 0
    }

    #[must_use]
    pub(crate) const fn passed(&self) -> usize {
        self.passed
    }

    #[must_use]
    pub(crate) const fn failed(&self) -> usize {
        self.failed
    }

    pub(crate) const fn add_failed(&mut self) {
        self.failed += 1;
        self.total += 1;
    }

    pub(crate) const fn add_passed(&mut self) {
        self.passed += 1;
        self.total += 1;
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

        writeln!(f, ". {} passed; {} failed", stats.passed(), stats.failed())
    }
}
