use crate::logging::Verbosity;
use clap::Parser;

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
    /// List of files or directories to test.
    #[clap(
        help = "List of files, directories, or test functions to test [default: the project root]",
        value_name = "PATH"
    )]
    pub paths: Vec<String>,

    #[clap(flatten)]
    pub(crate) verbosity: Verbosity,

    #[clap(
        long,
        short = 'p',
        help = "The prefix of the test functions",
        default_value = "test"
    )]
    pub(crate) test_prefix: String,
}
