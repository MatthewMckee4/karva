use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A unique identifier for a test run
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunHash {
    timestamp: u128,
}

impl RunHash {
    pub fn current_time() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time is before UNIX epoch")
            .as_millis();

        Self { timestamp }
    }

    pub fn from_existing(hash: &str) -> Self {
        let timestamp = hash
            .strip_prefix("run-")
            .unwrap_or(hash)
            .parse()
            .unwrap_or(0);
        Self { timestamp }
    }

    pub fn inner(&self) -> String {
        format!("run-{}", self.timestamp)
    }

    pub fn sort_key(&self) -> u128 {
        self.timestamp
    }
}

impl fmt::Display for RunHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner())
    }
}
