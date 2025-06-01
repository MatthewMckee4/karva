use clap::Parser;
use karva_project::project::ProjectOptions;

use crate::logging::Verbosity;

#[derive(Debug, Parser)]
#[command(author, name = "karva", about = "A Python test runner.")]
#[command(version)]
pub struct Args {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Run tests.
    Test(TestCommand),

    /// Display Karva's version
    Version,
}

#[derive(Debug, Parser)]
pub struct TestCommand {
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

    #[clap(long, help = "Run in watch mode", default_value = "false")]
    pub(crate) watch: bool,
}

impl TestCommand {
    pub fn into_options(self) -> ProjectOptions {
        ProjectOptions {
            test_prefix: self.test_prefix,
            watch: self.watch,
        }
    }
}
