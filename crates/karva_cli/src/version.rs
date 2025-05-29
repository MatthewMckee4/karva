use std::fmt;

pub struct VersionInfo {
    version: String,
}

impl fmt::Display for VersionInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.version)?;

        Ok(())
    }
}

pub fn version() -> VersionInfo {
    let version = option_env!("CARGO_PKG_VERSION")
        .map(ToString::to_string)
        .unwrap();

    VersionInfo { version }
}
