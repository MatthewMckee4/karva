use std::num::NonZeroU32;

use clap::{Parser, ValueEnum};

use crate::ShowConfigCommand;

/// Source-only suite inspection; execution requires an explicit runtime option.
#[derive(Debug, Parser)]
pub struct HealthCommand {
    /// Files, directories, globs, or test selectors, as accepted by `test`.
    pub paths: Vec<String>,

    #[command(flatten)]
    pub config: ShowConfigCommand,

    /// Run the selected tests and measure exclusive test and fixture lifecycle time.
    #[arg(long)]
    pub durations: bool,

    /// Repeat stable and seeded shuffled execution this many times each (at least 2).
    #[arg(long, value_name = "RUNS", conflicts_with = "durations")]
    pub probe: Option<NonZeroU32>,

    /// Reproducible shuffled order; repeat for more orders. Generated if omitted.
    #[arg(long, value_name = "SEED", requires = "probe")]
    pub probe_seed: Vec<u64>,

    /// Output representation. JSON uses a versioned schema.
    #[arg(long, value_enum, default_value = "text")]
    pub format: HealthFormat,

    /// Rule IDs to suppress for this invocation (comma separated).
    #[arg(long, value_delimiter = ',')]
    pub ignore: Vec<String>,

    /// Lowest finding severity that causes exit code 1.
    #[arg(long, value_enum, default_value = "error")]
    pub fail_on: HealthFailOn,
}

/// Stable health report representations.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum HealthFormat {
    Text,
    Json,
}

/// CI failure policy, independent of which diagnostics are displayed.
#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum HealthFailOn {
    None,
    Warning,
    Error,
}

/// Internal lifecycle observation policy; ordinary test invocations leave it disabled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HealthTimings {
    #[default]
    Disabled,
    Enabled,
}
