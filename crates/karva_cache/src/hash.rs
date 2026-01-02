use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// A unique identifier for a test run
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunHash(String);

impl RunHash {
    pub fn current_time() -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("System time is before UNIX epoch")
            .as_millis();

        Self(format!("run-{timestamp}"))
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
