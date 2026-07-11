use std::fmt;
use std::time::Instant;

use colored::Colorize;
use karva_logging::time::format_duration_bracketed;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::kind::TestResultKind;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TestResultStats {
    passed: usize,
    failed: usize,
    skipped: usize,
    flaky: usize,
    slow: usize,
}

impl TestResultStats {
    /// Total number of tests run. `Flaky` is a marker on a passing test and
    /// is not counted as a separate test.
    pub fn total(&self) -> usize {
        self.passed() + self.failed() + self.skipped()
    }

    pub fn is_success(&self) -> bool {
        self.failed() == 0
    }

    pub fn merge(&mut self, other: &Self) {
        self.passed += other.passed;
        self.failed += other.failed;
        self.skipped += other.skipped;
        self.flaky += other.flaky;
        self.slow += other.slow;
    }

    pub fn passed(&self) -> usize {
        self.passed
    }

    pub fn failed(&self) -> usize {
        self.failed
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

    pub fn add(&mut self, kind: TestResultKind) {
        match kind {
            TestResultKind::Passed => self.passed += 1,
            TestResultKind::Failed => self.failed += 1,
            TestResultKind::Skipped => self.skipped += 1,
            TestResultKind::Flaky => self.flaky += 1,
            TestResultKind::Slow => self.slow += 1,
        }
    }

    pub fn display(&self, start_time: Instant) -> DisplayTestResultStats<'_> {
        DisplayTestResultStats::new(self, start_time)
    }
}

impl Serialize for TestResultStats {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;

        let counts = [
            (TestResultKind::Passed, self.passed),
            (TestResultKind::Failed, self.failed),
            (TestResultKind::Skipped, self.skipped),
            (TestResultKind::Flaky, self.flaky),
            (TestResultKind::Slow, self.slow),
        ];
        let mut map = serializer
            .serialize_map(Some(counts.iter().filter(|(_, count)| *count > 0).count()))?;
        for (kind, count) in counts {
            if count > 0 {
                map.serialize_entry(kind.as_str(), &count)?;
            }
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
                let mut stats = TestResultStats::default();

                while let Some((key, value)) = access.next_entry::<String, usize>()? {
                    let kind = TestResultKind::from_str(&key).map_err(|_| {
                        de::Error::unknown_field(
                            &key,
                            &["passed", "failed", "skipped", "flaky", "slow"],
                        )
                    })?;
                    match kind {
                        TestResultKind::Passed => stats.passed = value,
                        TestResultKind::Failed => stats.failed = value,
                        TestResultKind::Skipped => stats.skipped = value,
                        TestResultKind::Flaky => stats.flaky = value,
                        TestResultKind::Slow => stats.slow = value,
                    }
                }

                Ok(stats)
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
    fn new(stats: &'a TestResultStats, start_time: Instant) -> Self {
        Self { stats, start_time }
    }
}

impl fmt::Display for DisplayTestResultStats<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let success = self.stats.is_success();
        let elapsed = self.start_time.elapsed();

        writeln!(f, "{}", "─".repeat(12))?;

        let label = format!("{:>12}", "Summary");
        if success {
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
        stats.add(TestResultKind::Skipped);

        let json = serde_json::to_string(&stats).unwrap();
        assert_eq!(json, r#"{"passed":2,"failed":1,"skipped":1}"#);
        let deserialized: TestResultStats = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.passed(), 2);
        assert_eq!(deserialized.failed(), 1);
        assert_eq!(deserialized.skipped(), 1);
        assert_eq!(deserialized.total(), 4);
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
    fn test_merge() {
        let mut a = TestResultStats::default();
        a.add(TestResultKind::Passed);

        let mut b = TestResultStats::default();
        b.add(TestResultKind::Passed);
        b.add(TestResultKind::Failed);

        a.merge(&b);
        assert_eq!(a.passed(), 2);
        assert_eq!(a.failed(), 1);
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
