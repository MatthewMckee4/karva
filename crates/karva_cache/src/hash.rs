use std::time::{SystemTime, UNIX_EPOCH};

use crate::models::RunHash;

/// Generate a unique run hash for this test execution
///
/// Format: "run-{unix_timestamp}-{random_u32}"
/// Example: "run-1703001234-a3f9c8d2"
pub fn generate_run_hash() -> RunHash {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System time is before UNIX epoch")
        .as_secs();

    // Use thread-local random number for uniqueness
    let random: u32 = rand::random();

    RunHash(format!("run-{timestamp:x}-{random:x}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_run_hash() {
        let hash1 = generate_run_hash();
        let hash2 = generate_run_hash();

        // Hashes should be different (very likely due to random component)
        assert_ne!(hash1, hash2);

        // Should start with "run-"
        assert!(hash1.0.starts_with("run-"));
        assert!(hash2.0.starts_with("run-"));

        // Should have the expected format
        let parts: Vec<&str> = hash1.0.split('-').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], "run");
        // parts[1] should be a timestamp (numeric)
        assert!(parts[1].parse::<u64>().is_ok());
        // parts[2] should be a hex random number
        assert!(u32::from_str_radix(parts[2], 16).is_ok());
    }
}
