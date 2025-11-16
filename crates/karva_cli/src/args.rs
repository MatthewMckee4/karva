use camino::Utf8PathBuf;
use clap::Parser;
use karva_project::project::ProjectOptions;

use crate::logging::Verbosity;

#[derive(Debug, Parser)]
#[command(author, name = "karva", about = "A Python test runner.")]
#[command(version)]
pub struct Args {
    #[command(subcommand)]
    pub(crate) command: Command,
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
    pub(crate) paths: Vec<Utf8PathBuf>,

    #[clap(flatten)]
    pub(crate) verbosity: Verbosity,

    #[clap(
        long,
        help = "The prefix of the test functions",
        default_value = "test"
    )]
    pub(crate) test_prefix: String,

    #[clap(short = 's', long, help = "Show Python stdout during test execution")]
    pub(crate) show_output: bool,

    #[clap(
        long,
        help = "When set, .gitignore files will not be respected.",
        default_value = "false"
    )]
    pub(crate) no_ignore: bool,

    #[clap(
        long,
        help = "When set, the test will fail immediately if any test fails.",
        default_value = "false"
    )]
    pub(crate) fail_fast: bool,
}

impl TestCommand {
    pub(crate) fn into_options(self) -> ProjectOptions {
        ProjectOptions::new(
            self.test_prefix,
            self.verbosity.level(),
            self.show_output,
            !self.no_ignore,
            self.fail_fast,
        )
    }
}
