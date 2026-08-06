use std::fmt;
use std::time::Instant;

use colored::Colorize;
use karva_logging::time::format_duration_bracketed;
use serde::{Deserialize, Serialize};

use super::kind::TestResultKind;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
/// Outcome counters for a run; flaky and slow are additional markers, not tests.
pub struct TestResultStats {
    #[serde(skip_serializing_if = "is_default")]
    passed: usize,

    #[serde(skip_serializing_if = "is_default")]
    failed: usize,

    #[serde(rename = "error", skip_serializing_if = "is_default")]
    errors: usize,

    #[serde(skip_serializing_if = "is_default")]
    skipped: usize,

    #[serde(skip_serializing_if = "is_default")]
    flaky: usize,

    #[serde(skip_serializing_if = "is_default")]
    slow: usize,
}

fn is_default<T: Default + PartialEq>(value: &T) -> bool {
    *value == T::default()
}

impl TestResultStats {
    /// Total number of tests run. `Flaky` is a marker on a passing test and
    /// is not counted as a separate test.
    pub fn total(&self) -> usize {
        self.passed() + self.failed() + self.errors() + self.skipped()
    }

    /// Whether no failed or errored tests were recorded.
    pub fn is_success(&self) -> bool {
        self.failed() == 0 && self.errors() == 0
    }

    pub fn passed(&self) -> usize {
        self.passed
    }

    pub fn failed(&self) -> usize {
        self.failed
    }

    pub fn errors(&self) -> usize {
        self.errors
    }

    pub fn skipped(&self) -> usize {
        self.skipped
    }

    pub fn flaky(&self) -> usize {
        self.flaky
    }

    pub fn slow(&self) -> usize {
        self.slow
    }

    /// Increments the counter represented by `kind`.
    pub(super) fn add(&mut self, kind: TestResultKind) {
        match kind {
            TestResultKind::Passed => self.passed += 1,
            TestResultKind::Failed => self.failed += 1,
            TestResultKind::Error => self.errors += 1,
            TestResultKind::Skipped => self.skipped += 1,
            TestResultKind::Flaky => self.flaky += 1,
            TestResultKind::Slow => self.slow += 1,
        }
    }

    /// Returns summary formatting using elapsed time since `start_time`.
    pub fn display(&self, start_time: Instant, success: bool) -> DisplayTestResultStats<'_> {
        DisplayTestResultStats::new(self, start_time, success)
    }
}

pub struct DisplayTestResultStats<'a> {
    stats: &'a TestResultStats,
    start_time: Instant,
    success: bool,
}

impl<'a> DisplayTestResultStats<'a> {
    fn new(stats: &'a TestResultStats, start_time: Instant, success: bool) -> Self {
        Self {
            stats,
            start_time,
            success,
        }
    }
}

impl fmt::Display for DisplayTestResultStats<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let elapsed = self.start_time.elapsed();

        writeln!(f, "{}", "─".repeat(12))?;

        let label = format!("{:>12}", "Summary");
        if self.success {
            write!(f, "{}", label.green().bold())?;
        } else {
            write!(f, "{}", label.red().bold())?;
        }

        let passed_text = if self.stats.flaky() > 0 {
            format!(
                "{} passed ({} flaky)",
                self.stats.passed(),
                self.stats.flaky()
            )
        } else {
            format!("{} passed", self.stats.passed())
        };
        let mut parts = vec![passed_text.green().bold().to_string()];
        if self.stats.failed() > 0 {
            parts.push(
                format!("{} failed", self.stats.failed())
                    .red()
                    .bold()
                    .to_string(),
            );
        }
        if self.stats.errors() > 0 {
            parts.push(
                format!(
                    "{} {}",
                    self.stats.errors(),
                    if self.stats.errors() == 1 {
                        "error"
                    } else {
                        "errors"
                    }
                )
                .red()
                .bold()
                .to_string(),
            );
        }
        parts.push(
            format!("{} skipped", self.stats.skipped())
                .yellow()
                .bold()
                .to_string(),
        );
        if self.stats.slow() > 0 {
            parts.push(
                format!("{} slow", self.stats.slow())
                    .yellow()
                    .bold()
                    .to_string(),
            );
        }

        writeln!(
            f,
            " {} {} {} run: {}",
            format_duration_bracketed(elapsed),
            self.stats.total(),
            if self.stats.total() == 1 {
                "test"
            } else {
                "tests"
            },
            parts.join(", "),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serde_roundtrip() {
        let mut stats = TestResultStats::default();
        stats.add(TestResultKind::Passed);
        stats.add(TestResultKind::Passed);
        stats.add(TestResultKind::Failed);
        stats.add(TestResultKind::Error);
        stats.add(TestResultKind::Skipped);
        stats.add(TestResultKind::Flaky);
        stats.add(TestResultKind::Slow);

        let json = serde_json::to_string(&stats).unwrap();
        assert_eq!(
            json,
            r#"{"passed":2,"failed":1,"error":1,"skipped":1,"flaky":1,"slow":1}"#
        );
        let deserialized: TestResultStats = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized, stats);
    }

    #[test]
    fn test_deserialize_empty() {
        let stats: TestResultStats = serde_json::from_str("{}").unwrap();
        assert_eq!(stats.passed(), 0);
        assert_eq!(stats.failed(), 0);
        assert_eq!(stats.skipped(), 0);
    }

    #[test]
    fn test_deserialize_partial() {
        let stats: TestResultStats = serde_json::from_str(r#"{"passed": 5}"#).unwrap();
        assert_eq!(stats.passed(), 5);
        assert_eq!(stats.failed(), 0);
        assert_eq!(stats.skipped(), 0);
    }

    #[test]
    fn test_deserialize_unknown_field() {
        let result = serde_json::from_str::<TestResultStats>(r#"{"invalid": 1}"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_is_success() {
        let mut stats = TestResultStats::default();
        assert!(stats.is_success());

        stats.add(TestResultKind::Passed);
        assert!(stats.is_success());

        stats.add(TestResultKind::Failed);
        assert!(!stats.is_success());
    }
}
