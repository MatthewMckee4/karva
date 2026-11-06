use camino::Utf8PathBuf;
use clap::{Parser, ValueEnum};

use crate::ShowConfigCommand;

/// Output representation for source-only suite health findings.
#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub enum CheckFormat {
    #[default]
    Text,
    Json,
}

/// Check suite health without importing or executing Python source.
#[derive(Debug, Parser)]
pub struct CheckCommand {
    /// Files or directories to check; defaults to the current project.
    pub paths: Vec<Utf8PathBuf>,

    /// Diagnostic output representation.
    #[arg(long, value_enum, default_value_t = CheckFormat::Text)]
    pub output_format: CheckFormat,

    #[command(flatten)]
    pub config: ShowConfigCommand,
}
