use clap::Parser;
use clap::builder::Styles;
use clap::builder::styling::{AnsiColor, Effects};

mod cache;
mod enums;
mod partition;
mod show_config;
mod snapshot;
mod test;
mod verbosity;

pub use cache::{CacheAction, CacheCommand};
pub use enums::{CovReport, NoTests, OutputFormat, RunIgnored};
pub use partition::PartitionSelection;
pub use show_config::ShowConfigCommand;
pub use snapshot::{
    SnapshotAction, SnapshotCommand, SnapshotDeleteArgs, SnapshotFilterArgs, SnapshotPruneArgs,
};
pub use test::{SubTestCommand, TestCommand};
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
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Run tests.
    Test(Box<TestCommand>),

    /// Manage snapshots created by `karva.assert_snapshot()`.
    Snapshot(SnapshotCommand),

    /// Manage the karva cache.
    Cache(CacheCommand),

    /// Print the resolved configuration karva would run with.
    ShowConfig(ShowConfigCommand),

    /// Display Karva's version
    Version,
}
