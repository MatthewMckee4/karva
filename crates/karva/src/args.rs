use crate::logging::Verbosity;
use clap::Parser;
use karva_core::db::path::SystemPathBuf;

#[derive(Debug, Parser)]
#[command(author, name = "karva", about = "A Python test runner.")]
#[command(version)]
pub(crate) struct Args {
    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Debug, clap::Subcommand)]
pub(crate) enum Command {
    /// Run tests.
    Test(TestCommand),

    /// Display Karva's version
    Version,
}

#[derive(Debug, Parser)]
pub(crate) struct TestCommand {
    /// List of files or directories to check.
    #[clap(
        help = "List of files or directories to check [default: the project root]",
        value_name = "PATH"
    )]
    pub paths: Vec<SystemPathBuf>,

    #[clap(flatten)]
    pub(crate) verbosity: Verbosity,
}
