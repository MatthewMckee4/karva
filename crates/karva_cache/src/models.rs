use std::fmt;

use serde::{Deserialize, Serialize};

/// A unique identifier for a test run
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunHash(pub String);

impl fmt::Display for RunHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Serializable test result statistics
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SerializableStats {
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl SerializableStats {
    pub fn total(&self) -> usize {
        self.passed + self.failed + self.skipped
    }

    pub fn is_success(&self) -> bool {
        self.failed == 0
    }

    /// Merge another stats object into this one
    pub fn merge(&mut self, other: &Self) {
        self.passed += other.passed;
        self.failed += other.failed;
        self.skipped += other.skipped;
    }
}

/// Sanitize a test path to be filesystem-safe
///
/// Replaces path separators with double underscores and :: with triple underscores.
/// Example: "tests/test_foo.py::TestClass::test_method[param]"
///       -> "tests__test_foo.py___TestClass___test_method[param]"
pub fn sanitize_test_path(path: &str) -> String {
    path.replace("::", "___").replace('/', "__").replace('\\', "__")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_test_path() {
        assert_eq!(
            sanitize_test_path("tests/test_foo.py::test_example"),
            "tests__test_foo.py___test_example"
        );

        assert_eq!(
            sanitize_test_path("tests/test_foo.py::TestClass::test_method[param1-param2]"),
            "tests__test_foo.py___TestClass___test_method[param1-param2]"
        );

        // Windows paths
        assert_eq!(
            sanitize_test_path("tests\\test_foo.py::test_example"),
            "tests__test_foo.py___test_example"
        );
    }

    #[test]
    fn test_serializable_stats() {
        let mut stats1 = SerializableStats {
            passed: 5,
            failed: 2,
            skipped: 1,
        };

        assert_eq!(stats1.total(), 8);
        assert!(!stats1.is_success());

        let stats2 = SerializableStats {
            passed: 3,
            failed: 0,
            skipped: 2,
        };

        stats1.merge(&stats2);
        assert_eq!(stats1.passed, 8);
        assert_eq!(stats1.failed, 2);
        assert_eq!(stats1.skipped, 3);
    }
}
