use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A unique identifier for a test run based on a millisecond timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunHash {
    timestamp: u128,
}

impl RunHash {
    /// Creates a new hash from the current system time.
    pub fn current_time() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time is before UNIX epoch")
            .as_millis();

        Self { timestamp }
    }

    /// Parses a hash from an existing run directory name (e.g. `run-1234`).
    ///
    /// Falls back to timestamp `0` if the input cannot be parsed.
    pub fn from_existing(hash: &str) -> Self {
        let timestamp = hash
            .strip_prefix("run-")
            .unwrap_or(hash)
            .parse()
            .unwrap_or(0);
        Self { timestamp }
    }

    /// Returns the string representation used as a directory name (e.g. `run-1234`).
    pub fn inner(&self) -> String {
        format!("run-{}", self.timestamp)
    }

    /// Returns the underlying timestamp, used for ordering runs chronologically.
    pub fn sort_key(&self) -> u128 {
        self.timestamp
    }
}

impl fmt::Display for RunHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_time_produces_valid_hash() {
        let hash = RunHash::current_time();
        let inner = hash.inner();
        assert!(inner.starts_with("run-"));
        assert!(hash.sort_key() > 0);
    }

    #[test]
    fn from_existing_roundtrips_with_inner() {
        let original = RunHash::current_time();
        let inner = original.inner();
        let restored = RunHash::from_existing(&inner);
        assert_eq!(original, restored);
    }

    #[test]
    fn from_existing_handles_missing_prefix() {
        let hash = RunHash::from_existing("not-a-number");
        assert_eq!(hash.sort_key(), 0);
    }

    #[test]
    fn from_existing_handles_invalid_input() {
        let hash = RunHash::from_existing("run-abc");
        assert_eq!(hash.sort_key(), 0);
    }

    #[test]
    fn sort_key_reflects_timestamp_ordering() {
        let earlier = RunHash::from_existing("run-100");
        let later = RunHash::from_existing("run-200");
        assert!(earlier.sort_key() < later.sort_key());
    }

    #[test]
    fn display_matches_inner() {
        let hash = RunHash::from_existing("run-42");
        assert_eq!(hash.to_string(), hash.inner());
        assert_eq!(hash.to_string(), "run-42");
    }
}
