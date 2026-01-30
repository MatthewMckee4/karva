use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::options::Options;

/// A `pyproject.toml` as specified in PEP 517.
#[derive(Deserialize, Serialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct PyProject {
    /// Tool-specific metadata.
    pub tool: Option<Tool>,
}

impl PyProject {
    pub(crate) fn karva(&self) -> Option<&Options> {
        self.tool.as_ref().and_then(|tool| tool.karva.as_ref())
    }
}

#[derive(Error, Debug)]
pub enum PyProjectError {
    #[error(transparent)]
    TomlSyntax(#[from] toml::de::Error),
}

impl PyProject {
    pub(crate) fn from_toml_str(content: &str) -> Result<Self, PyProjectError> {
        toml::from_str(content).map_err(PyProjectError::TomlSyntax)
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub struct Tool {
    pub karva: Option<Options>,
}
