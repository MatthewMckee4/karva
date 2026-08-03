use camino::Utf8PathBuf;
use karva_combine::Combine;

use super::{Config, Options, UnknownProfile};

#[derive(Debug, Default, PartialEq, Eq, Clone)]
/// Explicit config path, profile, and CLI options supplied by one invocation.
pub struct ProjectOptionsOverrides {
    /// Configuration file replacing automatic project discovery.
    config_file_override: Option<Utf8PathBuf>,

    /// Named profile selected from loaded configuration.
    profile: Option<String>,

    /// Highest-precedence options parsed from CLI arguments.
    options: Options,
}

impl ProjectOptionsOverrides {
    /// Creates invocation overrides using default profile selection.
    pub fn new(config_file_override: Option<Utf8PathBuf>, options: Options) -> Self {
        Self {
            config_file_override,
            profile: None,
            options,
        }
    }

    /// Sets named profile selection.
    #[must_use]
    pub fn with_profile(mut self, profile: Option<String>) -> Self {
        self.profile = profile;
        self
    }

    /// Resolve the requested profile from `config` and combine the CLI
    /// overrides on top.
    pub(crate) fn apply_to(&self, config: Config) -> Result<Options, UnknownProfile> {
        let resolved = config.resolve_profile(self.profile.as_deref())?;
        let disable_flaky_result_overrides = self
            .options
            .test
            .as_ref()
            .is_some_and(|test| test.flaky_result.is_some());
        let mut options = self.options.clone().combine(resolved);
        if disable_flaky_result_overrides {
            for override_options in &mut options.overrides {
                override_options.flaky_result = None;
            }
        }
        Ok(options)
    }
}
