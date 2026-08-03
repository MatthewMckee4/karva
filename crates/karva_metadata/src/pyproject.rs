use serde::{Deserialize, Serialize};
use thiserror::Error;

use camino::Utf8PathBuf;

use crate::options::{Config, CoverageOptions, DEFAULT_PROFILE};
use crate::settings::CoverageExcludePattern;

/// A `pyproject.toml` as specified in PEP 517.
#[derive(Deserialize, Serialize, Debug, Default, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct PyProject {
    /// Tool-specific metadata.
    tool: Option<Tool>,
}

impl PyProject {
    pub(super) fn karva(&self) -> Option<&Config> {
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
    pub(super) fn from_toml_str(content: &str) -> Result<Self, PyProjectError> {
        toml::from_str(content).map_err(PyProjectError::TomlSyntax)
    }
}

#[derive(Deserialize, Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
/// Tool-specific section of `pyproject.toml` relevant to Karva.
pub struct Tool {
    /// Parsed `[tool.karva]` configuration, when present.
    pub karva: Option<Config>,

    /// coverage.py-compatible settings Karva can consume without coverage.py.
    pub coverage: Option<CoveragePyOptions>,
}

#[derive(Deserialize, Serialize, Debug, Default, Clone, PartialEq, Eq)]
/// Supported subset of `[tool.coverage]`.
pub struct CoveragePyOptions {
    /// Report analysis settings.
    pub report: Option<CoveragePyReportOptions>,
}

#[derive(Deserialize, Serialize, Debug, Default, Clone, PartialEq, Eq)]
/// Supported subset of `[tool.coverage.report]`.
pub struct CoveragePyReportOptions {
    /// Regular expressions excluding matching source lines or clauses.
    #[serde(default, rename = "exclude_lines")]
    pub exclude_lines: Vec<CoverageExcludePattern>,
}

impl PyProject {
    pub(super) fn into_karva_config(self) -> Config {
        let mut config = self
            .tool
            .as_ref()
            .and_then(|tool| tool.karva.clone())
            .unwrap_or_default();
        self.apply_coverage_exclusions(&mut config);
        config
    }

    pub(super) fn apply_coverage_exclusions(&self, config: &mut Config) {
        let patterns = self
            .tool
            .as_ref()
            .and_then(|tool| tool.coverage.as_ref())
            .and_then(|coverage| coverage.report.as_ref())
            .map(|report| report.exclude_lines.clone())
            .unwrap_or_default();
        if !patterns.is_empty() {
            let options = config
                .profile
                .entry(DEFAULT_PROFILE.to_owned())
                .or_default();
            let coverage = options
                .coverage
                .get_or_insert_with(CoverageOptions::default);
            if coverage.exclude_lines.is_none() {
                coverage.exclude_lines = Some(patterns);
            }
        }
    }
}
