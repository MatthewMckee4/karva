use camino::Utf8PathBuf;
use clap::{
    Parser,
    builder::{
        Styles,
        styling::{AnsiColor, Effects},
    },
};
use karva_project::project::{ProjectOptions, TestPrefix};
use ruff_db::diagnostic::DiagnosticFormat;

use crate::logging::Verbosity;

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default());

#[derive(Debug, Parser)]
#[command(author, name = "karva", about = "A Python test runner.")]
#[command(version)]
#[command(styles = STYLES)]
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

#[allow(clippy::struct_excessive_bools)]
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

    /// The prefix of the test functions.
    #[clap(long, default_value = "test")]
    pub(crate) test_prefix: String,

    /// The format to use for printing diagnostic messages.
    #[arg(long, default_value = "full")]
    pub(crate) output_format: OutputFormat,

    /// Show Python stdout during test execution.
    #[clap(short = 's', long)]
    pub(crate) show_output: bool,

    /// When set, .gitignore files will not be respected.
    #[clap(long)]
    pub(crate) no_ignore: bool,

    /// When set, the test will fail immediately if any test fails.
    #[clap(long)]
    pub(crate) fail_fast: bool,

    /// When set, we will try to import functions in each test file as well as parsing the ast to find them.
    ///
    /// This is often slower, so it is not recommended for large projects.
    #[clap(long)]
    pub(crate) try_import_fixtures: bool,

    /// When set, we will show the traceback of each test failure.
    #[clap(long)]
    pub(crate) show_traceback: bool,
}

/// The diagnostic output format.
#[derive(Copy, Clone, Hash, Debug, PartialEq, Eq, PartialOrd, Ord, Default, clap::ValueEnum)]
pub enum OutputFormat {
    /// Print diagnostics verbosely, with context and helpful hints (default).
    #[default]
    #[value(name = "full")]
    Full,

    /// Print diagnostics concisely, one per line.
    #[value(name = "concise")]
    Concise,
}

impl From<OutputFormat> for DiagnosticFormat {
    fn from(value: OutputFormat) -> Self {
        match value {
            OutputFormat::Full => Self::Full,
            OutputFormat::Concise => Self::Concise,
        }
    }
}

impl TestCommand {
    pub(crate) fn into_options(self) -> ProjectOptions {
        ProjectOptions::new(
            TestPrefix::new(self.test_prefix),
            self.verbosity.level(),
            self.show_output,
            self.no_ignore,
            self.fail_fast,
            self.try_import_fixtures,
            self.show_traceback,
            self.output_format,
        )
    }
}
