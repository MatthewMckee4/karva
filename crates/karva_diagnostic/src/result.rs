use std::fmt::Debug;
use std::time::Instant;
use std::{collections::HashMap, fmt};

use colored::Colorize;
use karva_python_semantic::QualifiedTestName;
use karva_time::format_duration;
use ruff_db::diagnostic::Diagnostic;
use serde::de::{self, MapAccess};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Visitor};

use crate::reporter::Reporter;

/// Represents the result of a test run.
///
/// This is held in the test context and updated throughout the test run.
#[derive(Debug, Clone, Default)]
pub struct TestRunResult {
    /// Diagnostics generated during test discovery.
    discovery_diagnostics: Vec<Diagnostic>,

    /// Diagnostics generated during test collection and  execution.
    diagnostics: Vec<Diagnostic>,

    /// Stats generated during test execution.
    stats: TestResultStats,
}

impl TestRunResult {
    pub fn total_diagnostics(&self) -> usize {
        self.discovery_diagnostics.len() + self.diagnostics.len()
    }

    pub const fn diagnostics(&self) -> &Vec<Diagnostic> {
        &self.diagnostics
    }

    pub const fn discovery_diagnostics(&self) -> &Vec<Diagnostic> {
        &self.discovery_diagnostics
    }

    pub fn add_discovery_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.discovery_diagnostics.push(diagnostic);
    }

    pub fn add_diagnostic(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn is_success(&self) -> bool {
        self.stats().is_success() && self.discovery_diagnostics.is_empty()
    }

    pub const fn stats(&self) -> &TestResultStats {
        &self.stats
    }

    #[must_use]
    pub fn with_stats(mut self, stats: TestResultStats) -> Self {
        self.stats = stats;
        self
    }

    pub fn register_test_case_result(
        &mut self,
        test_case_name: &QualifiedTestName,
        result: IndividualTestResultKind,
        reporter: Option<&dyn Reporter>,
    ) {
        self.stats.add(result.clone().into());
        if let Some(reporter) = reporter {
            reporter.report_test_case_result(test_case_name, result);
        }
    }

    #[must_use]
    pub fn into_sorted(mut self) -> Self {
        self.diagnostics.sort_by(Diagnostic::ruff_start_ordering);
        self
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

impl TestResultKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    fn from_str(s: &str) -> Result<Self, &'static str> {
        match s {
            "passed" => Ok(Self::Passed),
            "failed" => Ok(Self::Failed),
            "skipped" => Ok(Self::Skipped),
            _ => Err("invalid TestResultKind"),
        }
    }
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

    pub fn merge(&mut self, other: &Self) {
        for (kind, count) in &other.inner {
            self.inner
                .entry(*kind)
                .and_modify(|v| *v += count)
                .or_insert(*count);
        }
    }

    pub fn passed(&self) -> usize {
        self.get(TestResultKind::Passed)
    }

    pub fn failed(&self) -> usize {
        self.get(TestResultKind::Failed)
    }

    pub fn skipped(&self) -> usize {
        self.get(TestResultKind::Skipped)
    }

    pub fn add(&mut self, kind: TestResultKind) {
        self.inner.entry(kind).and_modify(|v| *v += 1).or_insert(1);
    }

    pub const fn display(&self, start_time: Instant) -> DisplayTestResultStats<'_> {
        DisplayTestResultStats::new(self, start_time)
    }
}

impl Serialize for TestResultStats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        let mut map = serializer.serialize_map(Some(self.inner.len()))?;
        for (kind, count) in &self.inner {
            map.serialize_entry(kind.as_str(), count)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for TestResultStats {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StatsVisitor;

        impl<'de> Visitor<'de> for StatsVisitor {
            type Value = TestResultStats;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map of test result kinds to counts")
            }

            fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut inner = HashMap::new();

                while let Some((key, value)) = access.next_entry::<String, usize>()? {
                    let kind = TestResultKind::from_str(&key).map_err(|_| {
                        de::Error::unknown_field(&key, &["passed", "failed", "skipped"])
                    })?;
                    inner.insert(kind, value);
                }

                Ok(TestResultStats { inner })
            }
        }

        deserializer.deserialize_map(StatsVisitor)
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

        writeln!(
            f,
            ". {} passed; {} failed; {} skipped; finished in {}",
            self.stats.passed(),
            self.stats.failed(),
            self.stats.skipped(),
            format_duration(elapsed)
        )
    }
}
