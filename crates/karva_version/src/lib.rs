//! Build-time Karva version and commit metadata.

/// Return the application version.
///
/// This should be in sync with the release version based on the crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_version() {
        assert_eq!(version().to_string(), env!("CARGO_PKG_VERSION").to_string());
    }
}
