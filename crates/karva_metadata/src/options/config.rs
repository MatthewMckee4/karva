use std::collections::BTreeMap;

use camino::{Utf8Path, Utf8PathBuf};
use fs_err as fs;
use karva_combine::Combine;
use karva_macros::OptionsMetadata;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{DEFAULT_PROFILE, Options};

/// File-level configuration: a collection of named profiles.
///
/// Every option group lives inside `[profile.<name>]`. The implicit `default`
/// profile is always available; named profiles inherit from it and can
/// override individual fields.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, OptionsMetadata)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Config {
    /// `SemVer` requirement that the running karva binary must satisfy.
    ///
    /// When set, karva refuses to run if the installed version does not
    /// match the requirement. This is useful in CI and for shared
    /// repositories where every developer should be on a known-good
    /// version.
    #[option(
        default = r#"null"#,
        value_type = "string",
        example = r#"
            required-version = ">=0.5.0"
        "#
    )]
    #[cfg_attr(feature = "schemars", schemars(with = "Option<String>"))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    required_version: Option<VersionReq>,

    #[cfg_attr(feature = "schemars", schemars(schema_with = "profile_schema"))]
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    /// Named option profiles; `default` forms the base for every named profile.
    pub(crate) profile: BTreeMap<String, Options>,
}

#[cfg(feature = "schemars")]
fn profile_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let options = generator.subschema_for::<Options>();
    schemars::json_schema!({
        "type": "object",
        "propertyNames": {
            "pattern": "^[A-Za-z0-9_-]+$",
            "not": { "pattern": "^default-" }
        },
        "additionalProperties": options
    })
}

impl Config {
    /// Parses configuration text and validates all profile names.
    pub(crate) fn from_toml_str(content: &str) -> Result<Self, KarvaTomlError> {
        let config: Self = toml::from_str(content)?;
        validate_profile_names(&config.profile)?;
        Ok(config)
    }

    /// Verify that the running karva version satisfies `required-version`.
    ///
    /// `current` is parsed once with [`semver::Version::parse`]; karva's
    /// own version is well-formed semver, so a parse failure here is an
    /// internal error rather than a configuration problem.
    pub(crate) fn check_required_version(
        &self,
        current: &str,
    ) -> Result<(), IncompatibleVersionError> {
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
    #[cfg(test)]
    pub(super) fn has_profile(&self, name: &str) -> bool {
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
    pub(super) fn resolve_profile(mut self, name: Option<&str>) -> Result<Options, UnknownProfile> {
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
    /// Requested undefined profile name.
    name: String,

    /// Sorted profile names available to the user, including implicit `default`.
    available: Vec<String>,
}

#[derive(Debug, Error)]
/// Failure to compare running Karva version with `required-version`.
pub enum IncompatibleVersionError {
    /// Installed version does not satisfy configured requirement.
    #[error("the installed karva {installed} does not satisfy `required-version = \"{required}\"`")]
    Mismatch {
        /// Configured semantic-version requirement.
        required: VersionReq,

        /// Parsed running Karva version.
        installed: Version,
    },
    /// Build supplied a version string that is not valid semantic versioning.
    #[error("internal error: failed to parse installed karva {version}")]
    InvalidInstalledVersion {
        /// Invalid build version.
        version: String,

        /// Semantic-version parser failure.
        #[source]
        source: semver::Error,
    },
}

#[derive(Error, Debug)]
/// Failure while reading, decoding, or validating `karva.toml`.
pub enum KarvaTomlError {
    /// TOML syntax or schema decoding failed.
    #[error(transparent)]
    TomlSyntax(#[from] toml::de::Error),

    /// Configuration file could not be read.
    #[error("Failed to read `{path}`")]
    FileReadError {
        /// Underlying filesystem failure.
        #[source]
        source: std::io::Error,

        /// Configuration path that failed.
        path: Utf8PathBuf,
    },

    /// Profile name violates naming or reserved-prefix rules.
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
