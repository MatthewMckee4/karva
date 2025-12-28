use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A unique identifier for a test run
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunHash(String);

impl RunHash {
    pub fn random() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time is before UNIX epoch")
            .as_secs();

        let random: u32 = rand::random();

        Self(format!("run-{timestamp:x}-{random:x}"))
    }

    pub fn from_existing(hash: &str) -> Self {
        Self(hash.to_string())
    }

    pub fn inner(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for RunHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_run_hash() {
        let hash1 = RunHash::random();
        let hash2 = RunHash::random();

        assert_ne!(hash1, hash2);

        assert!(hash1.0.starts_with("run-"));
        assert!(hash2.0.starts_with("run-"));

        let parts: Vec<&str> = hash1.0.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "run");
    }
}
