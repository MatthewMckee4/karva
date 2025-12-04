use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use thiserror::Error;

use super::options::{KarvaTomlError, Options};
use crate::{KARVA_CONFIG_FILE_NAME, System, metadata::value::ValueSource};

/// A `ty.toml` configuration file with the options it contains.
pub struct ConfigurationFile {
    path: Utf8PathBuf,
    options: Options,
}

impl ConfigurationFile {
    pub(crate) fn from_path(
        path: Utf8PathBuf,
        system: &dyn System,
    ) -> Result<Self, ConfigurationFileError> {
        let karva_toml_str = system.read_to_string(&path).map_err(|source| {
            ConfigurationFileError::FileReadError {
                source,
                path: path.clone(),
            }
        })?;

        match Options::from_toml_str(&karva_toml_str, ValueSource::File(Arc::new(path.clone()))) {
            Ok(options) => Ok(Self { path, options }),
            Err(error) => Err(ConfigurationFileError::InvalidKarvaToml {
                source: Box::new(error),
                path,
            }),
        }
    }

    /// Loads the user-level configuration file if it exists.
    ///
    /// Returns `None` if the file does not exist or if the concept of user-level configurations
    /// doesn't exist on `system`.
    pub(crate) fn user(system: &dyn System) -> Result<Option<Self>, ConfigurationFileError> {
        let Some(configuration_directory) = system.user_config_directory() else {
            return Ok(None);
        };

        let ty_toml_path = configuration_directory
            .join("karva")
            .join(KARVA_CONFIG_FILE_NAME);

        tracing::debug!(
            "Searching for a user-level configuration at `{path}`",
            path = &ty_toml_path
        );

        let Ok(ty_toml_str) = system.read_to_string(&ty_toml_path) else {
            return Ok(None);
        };

        match Options::from_toml_str(
            &ty_toml_str,
            ValueSource::File(Arc::new(ty_toml_path.clone())),
        ) {
            Ok(options) => Ok(Some(Self {
                path: ty_toml_path,
                options,
            })),
            Err(error) => Err(ConfigurationFileError::InvalidKarvaToml {
                source: Box::new(error),
                path: ty_toml_path,
            }),
        }
    }

    /// Returns the path to the configuration file.
    pub(crate) fn path(&self) -> &Utf8Path {
        &self.path
    }

    pub(crate) fn into_options(self) -> Options {
        self.options
    }
}

#[derive(Debug, Error)]
pub enum ConfigurationFileError {
    #[error("{path} is not a valid `karva.toml`: {source}")]
    InvalidKarvaToml {
        source: Box<KarvaTomlError>,
        path: Utf8PathBuf,
    },
    #[error("Failed to read `{path}`: {source}")]
    FileReadError {
        #[source]
        source: std::io::Error,
        path: Utf8PathBuf,
    },
}
