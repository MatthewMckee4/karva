//! Command-line syntax shared by the controller and worker binaries.

use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::{AnsiColor, Effects};

mod cache;
mod coverage;
mod enums;
mod exit_status;
mod partition;
mod show_config;
mod snapshot;
mod test;
mod verbosity;

pub use cache::{CacheAction, CacheCommand};
pub use coverage::{
    CoverageAction, CoverageCombineCommand, CoverageCommand, CoverageFormat, CoverageHtmlCommand,
    CoverageJsonCommand, CoverageLcovCommand, CoverageReportCommand, CoverageSort,
    CoverageXmlCommand,
};
pub use enums::{
    CovContext, CovReport, FlakyResult, JunitFlakyFailStatus, NoTests, OutputFormat, ResultFormat,
    RunIgnored,
};
pub use exit_status::ExitStatus;
pub use partition::PartitionSelection;
pub use show_config::ShowConfigCommand;
pub use snapshot::{
    SnapshotAction, SnapshotCommand, SnapshotDeleteArgs, SnapshotFilterArgs, SnapshotPruneArgs,
};
pub use test::{RandomSeed, SubTestCommand, TestCommand};
pub use verbosity::Verbosity;

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default());

#[derive(Debug, Parser)]
#[command(author, name = "karva", about = "A Python test runner.")]
#[command(version = karva_version::version())]
#[command(styles = STYLES)]
/// Root command-line arguments for the `karva` executable.
pub struct Args {
    /// Top-level operation selected by the user.
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, clap::Subcommand)]
/// Top-level operations supported by the `karva` executable.
pub enum Command {
    /// Run tests.
    Test(Box<TestCommand>),

    /// Manage snapshots created by `karva.assert_snapshot()`.
    Snapshot(SnapshotCommand),

    /// Read and report native Karva coverage data.
    Coverage(CoverageCommand),

    /// Manage the karva cache.
    Cache(CacheCommand),

    /// Print the resolved configuration karva would run with.
    ShowConfig(ShowConfigCommand),

    /// Run the language server.
    Server,

    /// Generate shell completion.
    #[command(hide = true)]
    GenerateShellCompletion {
        /// The shell to generate the completion script for.
        shell: clap_complete_command::Shell,
    },

    /// Display Karva's version
    Version,
}
