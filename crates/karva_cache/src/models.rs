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

/// Sanitize a test path to be filesystem-safe
///
/// Replaces path separators with double underscores and :: with triple underscores.
/// Example: "`tests/test_foo.py::TestClass::test_method`"
///       -> "`tests__test_foo.py___TestClass___test_method`"
pub fn sanitize_test_path(path: &str) -> String {
    path.replace("::", "___").replace(['/', '\\'], "__")
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
}
