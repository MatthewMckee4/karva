use camino::Utf8PathBuf;
use clap::{Parser, ValueEnum};

use crate::ConfigArgs;

/// Output representation for source-only suite health findings.
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum CheckFormat {
    #[default]
    /// Source snippets with primary and related annotations.
    Text,
    /// Machine-readable diagnostics with byte and line/column locations.
    Json,
}

/// Check suite health without importing or executing Python source.
#[derive(Debug, Parser)]
pub struct CheckCommand {
    /// Files or directories to check; defaults to configured test paths.
    pub paths: Vec<Utf8PathBuf>,

    /// Diagnostic output representation.
    #[arg(long, value_enum, default_value_t = CheckFormat::Text)]
    pub output_format: CheckFormat,

    #[command(flatten)]
    pub config: ConfigArgs,
}
