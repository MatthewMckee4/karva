use std::collections::BTreeMap;

use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;
use karva_combine::Combine;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::Options;

/// The implicit name of the default profile.
pub const DEFAULT_PROFILE: &str = "default";

/// File-level configuration: a collection of named profiles.
///
/// Mirrors nextest: every option group lives inside `[profile.<name>]`. The
/// implicit `default` profile is always available; other profiles inherit
/// from it (and can override individual fields).
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    /// `SemVer` requirement that the running karva binary must satisfy.
    ///
    /// When set, karva refuses to run if the installed version does not
    /// match the requirement. This is useful in CI and for shared
    /// repositories where every developer should be on a known-good
    /// version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required_version: Option<VersionReq>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub profile: BTreeMap<String, Options>,
}

impl Config {
    pub fn from_toml_str(content: &str) -> Result<Self, KarvaTomlError> {
        let config: Self = toml::from_str(content)?;
        validate_profile_names(&config.profile)?;
        Ok(config)
    }

    /// Verify that the running karva version satisfies `required-version`.
    ///
    /// `current` is parsed once with [`semver::Version::parse`]; karva's
    /// own version is well-formed semver, so a parse failure here is an
    /// internal error rather than a configuration problem.
    pub fn check_required_version(&self, current: &str) -> Result<(), IncompatibleVersionError> {
        let Some(required) = &self.required_version else {
            return Ok(());
        };

        let installed = Version::parse(current).map_err(|source| {
            IncompatibleVersionError::InvalidInstalledVersion {
                version: current.to_string(),
                source,
            }
        })?;

        if required.matches(&installed) {
            Ok(())
        } else {
            Err(IncompatibleVersionError::Mismatch {
                required: required.clone(),
                installed,
            })
        }
    }

    pub(crate) fn from_karva_configuration_file(path: &Utf8Path) -> Result<Self, KarvaTomlError> {
        let karva_toml_str =
            fs::read_to_string(path).map_err(|source| KarvaTomlError::FileReadError {
                source,
                path: path.to_path_buf(),
            })?;

        Self::from_toml_str(&karva_toml_str)
    }

    /// Returns true if `name` is defined as a profile in this configuration.
    /// The implicit `default` profile always exists.
    pub fn has_profile(&self, name: &str) -> bool {
        if name == DEFAULT_PROFILE {
            return true;
        }
        self.profile.contains_key(name)
    }

    /// Resolve a profile by collapsing the `profile` map into a single
    /// [`Options`] value.
    ///
    /// The selected profile is layered on top of any `[profile.default]`
    /// overrides, which form the base. CLI options can then be combined with
    /// the result via the usual `Combine` precedence.
    ///
    /// Returns [`UnknownProfile`] when `name` refers to a profile that is
    /// not defined.
    pub fn resolve_profile(mut self, name: Option<&str>) -> Result<Options, UnknownProfile> {
        let requested = name.unwrap_or(DEFAULT_PROFILE);

        let default_overrides = self.profile.remove(DEFAULT_PROFILE);
        let named_overrides = if requested == DEFAULT_PROFILE {
            None
        } else if let Some(p) = self.profile.remove(requested) {
            Some(p)
        } else {
            let mut available: Vec<String> = self.profile.into_keys().collect();
            available.push(DEFAULT_PROFILE.to_string());
            available.sort();
            available.dedup();
            return Err(UnknownProfile {
                name: requested.to_string(),
                available,
            });
        };

        let mut effective = Options::default();
        if let Some(default_p) = default_overrides {
            effective = default_p.combine(effective);
        }
        if let Some(named_p) = named_overrides {
            effective = named_p.combine(effective);
        }
        Ok(effective)
    }
}

fn validate_profile_names(profiles: &BTreeMap<String, Options>) -> Result<(), KarvaTomlError> {
    for name in profiles.keys() {
        if name.is_empty() {
            return Err(KarvaTomlError::InvalidProfileName {
                name: name.clone(),
                reason: "profile name cannot be empty",
            });
        }
        if name != DEFAULT_PROFILE && name.starts_with("default-") {
            return Err(KarvaTomlError::InvalidProfileName {
                name: name.clone(),
                reason: "the `default-` prefix is reserved for built-in profiles",
            });
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(KarvaTomlError::InvalidProfileName {
                name: name.clone(),
                reason: "profile names may only contain ASCII letters, digits, `-`, and `_`",
            });
        }
    }
    Ok(())
}

#[derive(Debug, Error)]
#[error(
    "profile `{name}` is not defined in configuration (available: {})",
    available.join(", ")
)]
pub struct UnknownProfile {
    pub name: String,
    pub available: Vec<String>,
}

#[derive(Debug, Error)]
pub enum IncompatibleVersionError {
    #[error("the installed karva {installed} does not satisfy `required-version = \"{required}\"`")]
    Mismatch {
        required: VersionReq,
        installed: Version,
    },
    #[error("internal error: failed to parse installed karva {version}: {source}")]
    InvalidInstalledVersion {
        version: String,
        #[source]
        source: semver::Error,
    },
}

#[derive(Error, Debug)]
pub enum KarvaTomlError {
    #[error(transparent)]
    TomlSyntax(#[from] toml::de::Error),
    #[error("Failed to read `{path}`: {source}")]
    FileReadError {
        #[source]
        source: std::io::Error,
        path: Utf8PathBuf,
    },
    #[error("invalid profile name `{name}`: {reason}")]
    InvalidProfileName { name: String, reason: &'static str },
}

#[cfg(test)]
mod tests {
    use insta::assert_snapshot;

    use super::*;

    #[test]
    fn required_version_satisfied() {
        let config =
            Config::from_toml_str(r#"required-version = ">=0.0.1-alpha.1""#).expect("parse");
        config.check_required_version("0.0.1-alpha.5").expect("ok");
    }

    #[test]
    fn required_version_unsatisfied_reports_both_versions() {
        let config = Config::from_toml_str(r#"required-version = ">=1.0.0""#).expect("parse");
        let err = config
            .check_required_version("0.5.2")
            .expect_err("mismatch");
        assert_snapshot!(
            err,
            @r#"the installed karva 0.5.2 does not satisfy `required-version = ">=1.0.0"`"#
        );
    }

    #[test]
    fn required_version_absent_is_noop() {
        Config::default()
            .check_required_version("0.0.0")
            .expect("ok");
    }

    #[test]
    fn invalid_required_version_is_a_parse_error() {
        let err =
            Config::from_toml_str(r#"required-version = "not a version""#).expect_err("invalid");
        assert_snapshot!(err, @r#"
        TOML parse error at line 1, column 20
          |
        1 | required-version = "not a version"
          |                    ^^^^^^^^^^^^^^^
        unexpected character 'n' while parsing major version number

        "#);
    }
}
