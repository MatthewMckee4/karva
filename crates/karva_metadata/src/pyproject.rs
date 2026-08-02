use serde::{Deserialize, Serialize};
use thiserror::Error;

use camino::Utf8PathBuf;

use crate::options::Config;

/// A `pyproject.toml` as specified in PEP 517.
#[derive(Deserialize, Serialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct PyProject {
    /// Tool-specific metadata.
    pub tool: Option<Tool>,
}

impl PyProject {
    pub(crate) fn karva(&self) -> Option<&Config> {
        self.tool.as_ref().and_then(|tool| tool.karva.as_ref())
    }
}

#[derive(Error, Debug)]
/// Failure while reading or decoding `pyproject.toml`.
pub enum PyProjectError {
    /// TOML syntax or schema decoding failed.
    #[error(transparent)]
    TomlSyntax(#[from] toml::de::Error),

    /// File could not be read from disk.
    #[error("Failed to read `{path}`: {source}")]
    FileReadError {
        /// Underlying filesystem failure.
        #[source]
        source: std::io::Error,

        /// `pyproject.toml` path that failed.
        path: Utf8PathBuf,
    },
}

impl PyProject {
    pub(crate) fn from_toml_str(content: &str) -> Result<Self, PyProjectError> {
        toml::from_str(content).map_err(PyProjectError::TomlSyntax)
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
/// Tool-specific section of `pyproject.toml` relevant to Karva.
pub struct Tool {
    /// Parsed `[tool.karva]` configuration, when present.
    pub karva: Option<Config>,
}
